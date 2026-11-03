//! A study put onto a mesh: which ways each node is held, and what force is on each.
//!
//! This is what a solver is asked: a mesh, a material, held directions, forces on nodes, the
//! pull of the part's own weight and its being flung outward as it turns. Nothing here is a particular solver's: supports become
//! directions a node may not move along (so a fixed face, a sliding face and a pin are one
//! thing, and a node on two of them is held by both), and loads become forces on nodes, each
//! triangle's share worked out over its curved surface.

use std::collections::BTreeMap;

use tenon_geom::Vec3;
use tenon_model::study::{LoadKind, SupportKind};

use crate::mesh::{Line3, TetMesh};

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

/// A support of the study, on faces or edges of the part by their numbers.
#[derive(Clone, Debug, PartialEq)]
pub struct Held {
    pub name: String,
    pub kind: SupportKind,
    pub faces: Vec<u32>,
    pub edges: Vec<u32>,
}

/// A load of the study, on faces or edges of the part by their numbers.
#[derive(Clone, Debug, PartialEq)]
pub struct Pushed {
    pub name: String,
    pub kind: LoadKind,
    pub faces: Vec<u32>,
    pub edges: Vec<u32>,
}

/// The part turning: about the axis through `point` along `axis` (a unit vector), at `omega`
/// radians a second.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spin {
    pub point: Vec3,
    pub axis: Vec3,
    pub omega: f64,
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

/// How many pieces a mesh is in, and how many of them are not held fast.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Loose {
    pub pieces: usize,
    pub loose: usize,
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
    /// The part's turning, when the study has it.
    pub spin: Option<Spin>,
}

/// Adds `d` to a set of directions at right angles to each other, if it is not already
/// spanned by them.
fn hold(set: &mut Vec<Vec3>, d: Vec3) {
    let rest = set.iter().fold(d, |r, e| r - *e * r.dot(*e));
    if set.len() < 3 && rest.len() > 1e-6 {
        set.push(rest * (1.0 / rest.len()));
    }
}

/// A place on a loaded face where its load is summed up: where it is, its share of the face's
/// area as a vector out of the solid, what the face is, and the weight each node of its
/// triangle has there.
struct Spot {
    at: Vec3,
    da: Vec3,
    surface: Surface,
    nodes: [u32; 6],
    weights: [f64; 6],
}

/// Solves the three equations `m x = b` (None when they have no one answer).
fn solve3(m: [[f64; 3]; 3], b: Vec3) -> Option<Vec3> {
    let det = |a: [[f64; 3]; 3]| {
        a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1]) - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
            + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0])
    };
    let d = det(m);
    let scale = (m[0][0].abs() + m[1][1].abs() + m[2][2].abs()) / 3.0;
    if !(d.is_finite() && d.abs() > 1e-12 * scale * scale * scale && scale > 0.0) {
        return None;
    }
    let with = |col: usize| {
        let mut a = m;
        (a[0][col], a[1][col], a[2][col]) = (b.x, b.y, b.z);
        det(a) / d
    };
    Some(Vec3::new(with(0), with(1), with(2)))
}

/// The pushes on the spots that add up to `force` through their middle and to the moment that
/// `moment` gives about it (it is handed that middle): an even push, and a turning one, each
/// spot pushed as a rigid turn about the middle would move it. For a round bar's end that is
/// the shear of twisting; for a beam's end, the pull and push of bending.
fn force_and_moment(name: &str, spots: &[Spot], area: f64, force: Vec3, moment: impl FnOnce(Vec3) -> Vec3) -> Result<Vec<Vec3>, String> {
    let middle = spots.iter().fold(Vec3::ZERO, |sum, s| sum + s.at * s.da.len()) * (1.0 / area);
    let moment = moment(middle);
    // How the area lies about its middle (its moments of inertia).
    let mut inertia = [[0.0f64; 3]; 3];
    for s in spots {
        let (r, a) = (s.at - middle, s.da.len());
        let v = [r.x, r.y, r.z];
        for j in 0..3 {
            for k in 0..3 {
                inertia[j][k] += a * (if j == k { r.dot(r) } else { 0.0 } - v[j] * v[k]);
            }
        }
    }
    let turn = if moment.len() > 0.0 {
        solve3(inertia, moment).ok_or_else(|| format!("{name}: its faces are too slight to carry a turning load"))?
    } else {
        Vec3::ZERO
    };
    Ok(spots.iter().map(|s| (force * (1.0 / area) + turn.cross(s.at - middle)) * s.da.len()).collect())
}

/// The pushes of a bearing load: on each round face, on the half that the load pushes
/// against, square to the face, most where the face squarely faces the load and nothing at
/// the sides; scaled so that they add up to `newtons` along the load.
///
/// What is square to the face is worked out from where the spot is on the round face, not from
/// the triangle it is on: a small bore at a shape's mesh is a few flat sides (straight mid-side
/// nodes), and pushed square to those it would be pushed well off the way asked. On so few sides
/// a little across the load is still left over: that is taken back in shares of the pushes.
fn bearing(name: &str, spots: &[Spot], newtons: f64, direction: Vec3) -> Result<Vec<Vec3>, String> {
    // (A negative load presses the other way.)
    let (newtons, direction) = if newtons < 0.0 { (-newtons, -direction) } else { (newtons, direction) };
    // Each spot: how squarely it is pushed against, the way out of the solid there, its area, and
    // the way the load presses across its axis.
    let mut shares: Vec<(f64, Vec3, f64, Vec3)> = Vec::with_capacity(spots.len());
    let mut sum = 0.0;
    for s in spots {
        let Surface::Cylinder { origin, dir } = s.surface else {
            return Err(format!("{name}: a bearing load is on round (cylindrical) faces, and one of its faces is not"));
        };
        let across = direction - dir * direction.dot(dir);
        if across.len() <= 1e-9 * direction.len() {
            return Err(format!("{name}: a bearing load presses across the axis of its face, and its direction is along it"));
        }
        let (d, a) = (across.normalized(), s.da.len());
        // Square to the round face, on the side the triangle says is out of the solid (toward the
        // axis in a bore, away from it on a boss).
        let radial = (s.at - origin) - dir * (s.at - origin).dot(dir);
        let side = s.da.dot(radial);
        let out = if a == 0.0 {
            Vec3::ZERO
        } else if side != 0.0 {
            radial.normalized() * side.signum()
        } else {
            s.da * (1.0 / a)
        };
        // How squarely this bit of the face is pushed against (0 when it faces away).
        let facing = (-out.dot(d)).max(0.0);
        sum += facing * facing * a;
        shares.push((facing, out, a, d));
    }
    if sum <= 0.0 {
        return Err(format!("{name}: none of its faces is pushed against by a load that way"));
    }
    let k = newtons / sum;
    let mut pushes: Vec<Vec3> = shares.iter().map(|(facing, out, a, _)| *out * (-facing * a * k)).collect();
    // What the pushes come to, less what each was to come to along the load.
    let over = shares.iter().zip(&pushes).fold(Vec3::ZERO, |sum, ((facing, _, a, d), push)| sum + *push - *d * (facing * facing * a * k));
    let whole: f64 = pushes.iter().map(|p| p.len()).sum();
    if whole > 0.0 {
        pushes.iter_mut().for_each(|p| *p = *p - over * (p.len() / whole));
    }
    Ok(pushes)
}

/// The axis a moment turns about when none is said: its faces' own (a flat face's normal, a
/// round face's axis), which they must share.
fn own_axis(name: &str, surfaces: impl Iterator<Item = Surface>) -> Result<Vec3, String> {
    let mut found: Option<Vec3> = None;
    for surface in surfaces {
        let axis = match surface {
            Surface::Plane { normal } => normal,
            Surface::Cylinder { dir, .. } => dir,
            Surface::Other => {
                return Err(format!(
                    "{name}: one of its faces is neither flat nor round, and has no axis of its own: say which axis the moment turns about"
                ));
            }
        };
        match found {
            None => found = Some(axis.normalized()),
            Some(first) if first.dot(axis.normalized()).abs() < 0.999 => {
                return Err(format!("{name}: its faces do not share an axis: say which axis the moment turns about"));
            }
            Some(_) => {}
        }
    }
    found.ok_or_else(|| format!("{name}: none of the mesh lies on its faces"))
}

impl Problem {
    /// Puts a study onto the mesh. `on` says which face of the part each surface of the mesh
    /// is, and `curves` which edge of the part each curve of the mesh lies along.
    pub fn build(mesh: TetMesh, on: &BTreeMap<u32, u32>, curves: &BTreeMap<u32, u32>, setup: &Setup) -> Result<Problem, String> {
        let face_of = |surface: u32| on.get(&surface).copied();
        let edge_of = |curve: u32| curves.get(&curve).copied();
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
            // Along edges: a fixed support holds every node of them.
            for x in mesh.lines.iter().filter(|x| edge_of(x.curve).is_some_and(|e| s.edges.contains(&e))) {
                if s.kind != SupportKind::Fixed {
                    return Err(format!("{}: only a fixed support can be on edges", s.name));
                }
                any = true;
                for n in x.nodes {
                    let set = held.entry(n).or_default();
                    [Vec3::X, Vec3::Y, Vec3::Z].into_iter().for_each(|d| hold(set, d));
                }
            }
            if !any {
                return Err(format!("{}: none of the mesh lies on its faces", s.name));
            }
        }
        held.retain(|_, set| !set.is_empty());

        // ---- loads: the force on each node
        let mut forces: BTreeMap<u32, Vec3> = BTreeMap::new();
        let (mut gravity, mut spin) = (None, None);
        let mut put = |node: u32, f: Vec3| {
            let slot = forces.entry(node).or_insert(Vec3::ZERO);
            *slot = *slot + f;
        };
        for l in &setup.pushed {
            // On the whole part: left to the solver, which has the part's mass.
            match l.kind {
                LoadKind::Gravity { direction, acceleration } => {
                    gravity = Some(direction.normalized() * acceleration);
                    continue;
                }
                LoadKind::Body { axis, point, rpm } => {
                    spin = Some(Spin { point, axis: axis.normalized(), omega: rpm * std::f64::consts::TAU / 60.0 });
                    continue;
                }
                _ => {}
            }
            // Along edges: a force spread evenly over their length.
            if !l.edges.is_empty() {
                let LoadKind::Force { newtons, direction: Some(d) } = l.kind else {
                    return Err(format!("{}: only a force with a direction can be along edges", l.name));
                };
                let on_it: Vec<&Line3> = mesh.lines.iter().filter(|x| edge_of(x.curve).is_some_and(|e| l.edges.contains(&e))).collect();
                let length: f64 = on_it.iter().map(|x| mesh.length(x)).sum();
                if length <= 0.0 {
                    return Err(format!("{}: none of the mesh lies along its edges", l.name));
                }
                let per_length = d.normalized() * (newtons / length);
                for x in on_it {
                    for (dl, weights) in mesh.along(x) {
                        for (k, n) in x.nodes.iter().enumerate() {
                            put(*n, per_length * (dl * weights[k]));
                        }
                    }
                }
                continue;
            }
            // On faces: the spots of their triangles where the load is summed up.
            let mut spots: Vec<Spot> = Vec::new();
            for (i, t) in mesh.tris.iter().enumerate() {
                let Some(face) = face_of(t.surface).filter(|f| l.faces.contains(f)) else { continue };
                let p = t.nodes.map(|n| mesh.nodes[n as usize]);
                for (da, weights) in mesh.quadrature(t) {
                    let at = (0..6).fold(Vec3::ZERO, |sum, k| sum + p[k] * weights[k]);
                    // (da is square to the surface as the triangle winds: `outward` turns it
                    // out of the solid.)
                    spots.push(Spot { at, da: da * outward[i], surface: surface_of(face), nodes: t.nodes, weights });
                }
            }
            let area: f64 = spots.iter().map(|s| s.da.len()).sum();
            if area <= 0.0 {
                return Err(format!("{}: none of the mesh lies on its faces", l.name));
            }
            // What pushes on each spot: along one direction, square to the surface and into
            // it, or as its kind of load spreads it.
            let pushes: Vec<Vec3> = match l.kind {
                LoadKind::Force { newtons, direction: Some(d) } => {
                    let per_area = d.normalized() * (newtons / area);
                    spots.iter().map(|s| per_area * s.da.len()).collect()
                }
                LoadKind::Force { newtons, direction: None } => spots.iter().map(|s| s.da * (-newtons / area)).collect(),
                LoadKind::Pressure { megapascals } => spots.iter().map(|s| s.da * -megapascals).collect(),
                LoadKind::Moment { newton_millimetres, axis } => {
                    let axis = match axis {
                        Some(a) => a.normalized(),
                        None => own_axis(&l.name, spots.iter().map(|s| s.surface))?,
                    };
                    force_and_moment(&l.name, &spots, area, Vec3::ZERO, |_| axis * newton_millimetres)?
                }
                LoadKind::RemoteForce { newtons, direction, point } => {
                    let force = direction.normalized() * newtons;
                    force_and_moment(&l.name, &spots, area, force, |middle| (point - middle).cross(force))?
                }
                LoadKind::Bearing { newtons, direction } => bearing(&l.name, &spots, newtons, direction)?,
                LoadKind::Gravity { .. } | LoadKind::Body { .. } => Vec::new(),
            };
            for (s, push) in spots.iter().zip(pushes) {
                for (k, n) in s.nodes.iter().enumerate() {
                    put(*n, push * s.weights[k]);
                }
            }
        }
        Ok(Problem { mesh, material: setup.material, held, forces, gravity, spin })
    }

    /// The whole turning of the forces on nodes about `about`.
    pub fn total_moment(&self, about: Vec3) -> Vec3 {
        self.forces.iter().fold(Vec3::ZERO, |sum, (n, f)| sum + (self.mesh.nodes[*n as usize] - about).cross(*f))
    }

    /// The whole of the forces on nodes (not the part's weight).
    pub fn total_force(&self) -> Vec3 {
        self.forces.values().fold(Vec3::ZERO, |sum, f| sum + *f)
    }

    /// How many ways the part can still move as a whole, held as it is: 0 when it is held
    /// fast; up to 6 (three ways to slide, three to turn) when nothing holds it. A part that
    /// can still move has no one answer.
    pub fn free_motions(&self) -> usize {
        self.free_motions_of(&self.mesh.nodes, &mut self.held.iter())
    }

    /// How many pieces the mesh is in (those no element joins), and how many of them can still
    /// move as a whole. A bore wider than a plate leaves a part in two pieces that is still one
    /// solid to the kernel: with one piece held fast [`Problem::free_motions`] is 0, and the
    /// other floats, with no one answer.
    pub fn loose_pieces(&self) -> Loose {
        fn root(parent: &mut [u32], mut n: u32) -> u32 {
            while parent[n as usize] != n {
                let up = parent[parent[n as usize] as usize];
                parent[n as usize] = up;
                n = up;
            }
            n
        }
        // The nodes of one element are of one piece.
        let mut parent: Vec<u32> = (0..self.mesh.nodes.len() as u32).collect();
        for t in &self.mesh.tets {
            let first = root(&mut parent, t[0]);
            for n in &t[1..] {
                let other = root(&mut parent, *n);
                parent[other as usize] = first;
            }
        }
        // Each piece: its nodes, and what holds them. (A node of no element is of no piece.)
        let mut of_piece: BTreeMap<u32, (Vec<Vec3>, Vec<(&u32, &Vec<Vec3>)>)> = BTreeMap::new();
        let mut seen = vec![false; self.mesh.nodes.len()];
        for n in self.mesh.tets.iter().flatten() {
            if !std::mem::replace(&mut seen[*n as usize], true) {
                of_piece.entry(root(&mut parent, *n)).or_default().0.push(self.mesh.nodes[*n as usize]);
            }
        }
        for (n, dirs) in &self.held {
            if let Some(piece) = of_piece.get_mut(&root(&mut parent, *n)) {
                piece.1.push((n, dirs));
            }
        }
        let loose = of_piece.values().filter(|(nodes, held)| self.free_motions_of(nodes, &mut held.iter().copied()) > 0).count();
        Loose { pieces: of_piece.len(), loose }
    }

    /// How many ways what has the nodes `nodes` can still move as a whole, held by `held`.
    fn free_motions_of<'a>(&self, nodes: &[Vec3], held: &mut dyn Iterator<Item = (&'a u32, &'a Vec<Vec3>)>) -> usize {
        // A move of the whole part is a slide t and a turn w about a point c: a node at p
        // moves by t + w x (p - c). Held along d, it gives d.t + (r x d).w = 0. The ways left
        // are six less the number of such conditions that are independent.
        let Some(first) = nodes.first() else { return 6 };
        let (lo, hi) = nodes.iter().fold((*first, *first), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
        let (c, size) = ((lo + hi) * 0.5, (hi - lo).len().max(1e-9));
        let mut rows: Vec<[f64; 6]> = Vec::new();
        for (n, dirs) in held {
            // (Lengths scaled by its size, so that sliding and turning weigh alike.)
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

    /// The mesher's curves 1 to 3 (along x, the slanted side of the bottom, along y) as the
    /// part's edges 0 to 2.
    fn curves() -> BTreeMap<u32, u32> {
        (1..=3).map(|c| (c, c - 1)).collect()
    }

    fn build(mesh: TetMesh, on: &BTreeMap<u32, u32>, setup: &Setup) -> Result<Problem, String> {
        Problem::build(mesh, on, &curves(), setup)
    }

    fn steel() -> Material {
        Material::of(200.0, 0.29, 7.85)
    }

    fn setup(held: Vec<Held>, pushed: Vec<Pushed>, surfaces: Vec<Surface>) -> Setup {
        Setup { held, pushed, surfaces, material: steel() }
    }

    fn fixed(faces: &[u32]) -> Held {
        Held { name: "Fixed1".into(), kind: SupportKind::Fixed, faces: faces.to_vec(), edges: Vec::new() }
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
        let push = Pushed {
            name: "Force1".into(),
            kind: LoadKind::Force { newtons: 30.0, direction: Some(Vec3::new(0.0, 0.0, -2.0)) },
            faces: vec![3],
            edges: Vec::new(),
        };
        let p = build(mesh, &on, &setup(vec![fixed(&[0])], vec![push], surfaces)).unwrap();
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
        let press = Pushed { name: "Pressure1".into(), kind: LoadKind::Pressure { megapascals: 4.0 }, faces: vec![1], edges: Vec::new() };
        let p = build(mesh.clone(), &on, &setup(vec![fixed(&[0])], vec![press], surfaces.clone())).unwrap();
        assert!(near(p.total_force(), Vec3::new(0.0, 2.0, 0.0)), "{:?}", p.total_force());
        // A force of 6 N square to the slanted face: into the part, along -(1, 1, 1).
        let force = Pushed { name: "Force1".into(), kind: LoadKind::Force { newtons: 6.0, direction: None }, faces: vec![3], edges: Vec::new() };
        let p = build(mesh.clone(), &on, &setup(vec![fixed(&[0])], vec![force], surfaces.clone())).unwrap();
        assert!(near(p.total_force(), Vec3::new(1.0, 1.0, 1.0).normalized() * -6.0), "{:?}", p.total_force());
        // A negative one pulls. Spread over two faces it is shared by their areas.
        let pull = Pushed { name: "Force1".into(), kind: LoadKind::Force { newtons: -8.0, direction: None }, faces: vec![1, 2], edges: Vec::new() };
        let p = build(mesh.clone(), &on, &setup(vec![fixed(&[0])], vec![pull], surfaces.clone())).unwrap();
        assert!(near(p.total_force(), Vec3::new(-4.0, -4.0, 0.0)), "{:?}", p.total_force());
        // The same whichever way the mesher wound the triangle.
        let mut wound = mesh;
        wound.tris[1].nodes = [0, 3, 1, 7, 9, 4];
        let press = Pushed { name: "Pressure1".into(), kind: LoadKind::Pressure { megapascals: 4.0 }, faces: vec![1], edges: Vec::new() };
        let p = build(wound, &on, &setup(vec![fixed(&[0])], vec![press], surfaces)).unwrap();
        assert!(near(p.total_force(), Vec3::new(0.0, 2.0, 0.0)), "{:?}", p.total_force());
    }

    #[test]
    fn the_parts_weight_is_an_acceleration_and_not_forces_on_nodes() {
        let (mesh, on, surfaces) = tet();
        let weight = Pushed {
            name: "Gravity1".into(),
            kind: LoadKind::Gravity { direction: Vec3::new(0.0, 0.0, -3.0), acceleration: 9810.0 },
            faces: vec![],
            edges: Vec::new(),
        };
        let p = build(mesh, &on, &setup(vec![fixed(&[0])], vec![weight], surfaces)).unwrap();
        assert_eq!((p.gravity, p.forces.len()), (Some(Vec3::new(0.0, 0.0, -9810.0)), 0));
    }

    #[test]
    fn supports_are_directions_a_node_may_not_move_along() {
        let (mesh, on, surfaces) = tet();
        let slide = |faces: &[u32]| Held { name: "Frictionless1".into(), kind: SupportKind::Frictionless, faces: faces.to_vec(), edges: Vec::new() };
        // Fixed on z = 0: its six nodes held every way; the part cannot move.
        let p = build(mesh.clone(), &on, &setup(vec![fixed(&[0])], vec![], surfaces.clone())).unwrap();
        assert_eq!(p.held.len(), 6);
        assert!(p.held.values().all(|d| d.len() == 3));
        assert_eq!(p.free_motions(), 0);
        // Sliding on z = 0: held along z only. It can still slide two ways and turn one.
        let p = build(mesh.clone(), &on, &setup(vec![slide(&[0])], vec![], surfaces.clone())).unwrap();
        assert!(p.held.values().all(|d| d.len() == 1 && near(d[0], -Vec3::Z)));
        assert_eq!(p.free_motions(), 3);
        // Sliding on z = 0 and on y = 0: the nodes of the edge they share are held both ways.
        // The part can only slide along x.
        let p = build(mesh.clone(), &on, &setup(vec![slide(&[0, 1])], vec![], surfaces.clone())).unwrap();
        assert_eq!(p.held[&0].len(), 2, "the corner on both faces");
        assert_eq!(p.held[&2].len(), 1, "a corner on one");
        assert_eq!(p.free_motions(), 1);
        // On three faces at right angles: held fast, though no node is fixed but the corner.
        let p = build(mesh.clone(), &on, &setup(vec![slide(&[0, 1, 2])], vec![], surfaces.clone())).unwrap();
        assert_eq!((p.held[&0].len(), p.free_motions()), (3, 0));
        // Nothing held: six ways.
        let p = build(mesh.clone(), &on, &setup(vec![], vec![], surfaces.clone())).unwrap();
        assert_eq!(p.free_motions(), 6);
        // A support with none of the mesh on its faces says so.
        let e = build(mesh, &on, &setup(vec![fixed(&[9])], vec![], surfaces)).unwrap_err();
        assert!(e.contains("Fixed1: none of the mesh lies on its faces"), "{e}");
    }

    #[test]
    fn a_pin_holds_across_along_or_round_its_axis() {
        let (mesh, on, mut surfaces) = tet();
        // (For the test, the face at z = 0 said to be round about the z axis through 0.25, 0.25.)
        surfaces[0] = Surface::Cylinder { origin: Vec3::new(0.25, 0.25, 0.0), dir: Vec3::Z };
        let pin = |radial, axial, tangential| Held {
            name: "Pin1".into(),
            kind: SupportKind::Pin { radial, axial, tangential },
            faces: vec![0],
            edges: Vec::new(),
        };
        let p = build(mesh.clone(), &on, &setup(vec![pin(true, true, false)], vec![], surfaces.clone())).unwrap();
        // Node 1 at (1, 0, 0): across the pin is along (0.75, -0.25, 0); along it is z.
        let d = &p.held[&1];
        assert_eq!(d.len(), 2);
        assert!(near(d[0], Vec3::new(0.75, -0.25, 0.0).normalized()) && near(d[1], Vec3::Z), "{d:?}");
        // Free to turn: one way left.
        assert_eq!(p.free_motions(), 1);
        // Held from turning too: none.
        let p = build(mesh.clone(), &on, &setup(vec![pin(true, true, true)], vec![], surfaces.clone())).unwrap();
        assert_eq!((p.held[&1].len(), p.free_motions()), (3, 0));
        // Along only: it slides across and turns.
        let p = build(mesh.clone(), &on, &setup(vec![pin(false, true, false)], vec![], surfaces.clone())).unwrap();
        assert!(p.held.values().all(|d| d.len() == 1 && near(d[0], Vec3::Z)));
        // On a flat face a pin is refused, and a sliding support on a face that is neither.
        let flat = tet().2;
        assert!(build(mesh.clone(), &on, &setup(vec![pin(true, true, false)], vec![], flat)).unwrap_err().contains("a pin holds round"));
        surfaces[0] = Surface::Other;
        let slide = Held { name: "Frictionless1".into(), kind: SupportKind::Frictionless, faces: vec![0], edges: Vec::new() };
        assert!(build(mesh, &on, &setup(vec![slide], vec![], surfaces)).unwrap_err().contains("flat or round"));
    }

    #[test]
    fn a_fixed_support_and_a_force_can_be_along_edges() {
        let (mesh, on, surfaces) = tet();
        // Fixed along the part's edge 0 (the mesher's curve 1, along x): its three nodes.
        let along_x = Held { name: "Fixed1".into(), kind: SupportKind::Fixed, faces: vec![], edges: vec![0] };
        let p = build(mesh.clone(), &on, &setup(vec![along_x.clone()], vec![], surfaces.clone())).unwrap();
        assert_eq!(p.held.keys().copied().collect::<Vec<_>>(), [0, 1, 4]);
        assert!(p.held.values().all(|d| d.len() == 3));
        // (Held along one line, the part can still turn about it.)
        assert_eq!(p.free_motions(), 1);
        // 12 N down along the edge along y (edge 2, length 1): a sixth on each end, two thirds
        // on the middle.
        let push =
            Pushed { name: "Force1".into(), kind: LoadKind::Force { newtons: 12.0, direction: Some(-Vec3::Z) }, faces: vec![], edges: vec![2] };
        let p = build(mesh.clone(), &on, &setup(vec![fixed(&[0])], vec![push], surfaces.clone())).unwrap();
        assert!(near(p.total_force(), Vec3::new(0.0, 0.0, -12.0)), "{:?}", p.total_force());
        assert!(
            near(p.forces[&0], Vec3::new(0.0, 0.0, -2.0))
                && near(p.forces[&2], Vec3::new(0.0, 0.0, -2.0))
                && near(p.forces[&6], Vec3::new(0.0, 0.0, -8.0)),
            "{:?}",
            p.forces
        );
        // What cannot be along an edge says so; so does an edge the mesh has nothing along.
        let pin = Held { name: "Pin1".into(), kind: SupportKind::PIN, faces: vec![], edges: vec![0] };
        assert!(build(mesh.clone(), &on, &setup(vec![pin], vec![], surfaces.clone())).unwrap_err().contains("only a fixed support can be on edges"));
        let square = Pushed { name: "Force1".into(), kind: LoadKind::Force { newtons: 1.0, direction: None }, faces: vec![], edges: vec![2] };
        assert!(
            build(mesh.clone(), &on, &setup(vec![fixed(&[0])], vec![square], surfaces.clone()))
                .unwrap_err()
                .contains("only a force with a direction")
        );
        let nowhere =
            Pushed { name: "Force1".into(), kind: LoadKind::Force { newtons: 1.0, direction: Some(Vec3::X) }, faces: vec![], edges: vec![9] };
        assert!(build(mesh, &on, &setup(vec![fixed(&[0])], vec![nowhere], surfaces)).unwrap_err().contains("none of the mesh lies along its edges"));
    }

    #[test]
    fn a_moment_turns_about_the_middle_of_its_faces_and_adds_up_to_no_force() {
        let (mesh, on, surfaces) = tet();
        // The middle of the bottom face (z = 0, a right triangle with legs 1).
        let middle = Vec3::new(1.0 / 3.0, 1.0 / 3.0, 0.0);
        // 5 N mm about z on the bottom face: shear that turns it. No force in all; 5 about z.
        let turn = |axis: Option<Vec3>| Pushed {
            name: "Moment1".into(),
            kind: LoadKind::Moment { newton_millimetres: 5.0, axis },
            faces: vec![0],
            edges: Vec::new(),
        };
        let p = build(mesh.clone(), &on, &setup(vec![fixed(&[3])], vec![turn(Some(Vec3::new(0.0, 0.0, 2.0)))], surfaces.clone())).unwrap();
        assert!(p.total_force().len() < 1e-12, "{:?}", p.total_force());
        assert!(near(p.total_moment(middle), Vec3::new(0.0, 0.0, 5.0)), "{:?}", p.total_moment(middle));
        assert!(p.forces.values().all(|f| f.z.abs() < 1e-12), "all of it in the face");
        // With no axis said: the face's own, its normal, which looks down (-z).
        let p = build(mesh.clone(), &on, &setup(vec![fixed(&[3])], vec![turn(None)], surfaces.clone())).unwrap();
        assert!(near(p.total_moment(middle), Vec3::new(0.0, 0.0, -5.0)), "{:?}", p.total_moment(middle));
        // About an axis in the face: pull on one side of it and push on the other (bending),
        // square to the face. Still no force, and the whole moment.
        let p = build(mesh.clone(), &on, &setup(vec![fixed(&[3])], vec![turn(Some(Vec3::X))], surfaces.clone())).unwrap();
        assert!(
            p.total_force().len() < 1e-12 && near(p.total_moment(middle), Vec3::new(5.0, 0.0, 0.0)),
            "{:?} {:?}",
            p.total_force(),
            p.total_moment(middle)
        );
        assert!(p.forces.values().all(|f| f.x.abs() < 1e-12 && f.y.abs() < 1e-12), "all of it square to the face");
        // Two faces that share no axis need one said.
        let both =
            Pushed { name: "Moment1".into(), kind: LoadKind::Moment { newton_millimetres: 5.0, axis: None }, faces: vec![0, 1], edges: Vec::new() };
        assert!(build(mesh, &on, &setup(vec![fixed(&[3])], vec![both], surfaces)).unwrap_err().contains("do not share an axis"));
    }

    #[test]
    fn a_remote_force_is_its_force_and_its_turning_about_the_faces() {
        let (mesh, on, surfaces) = tet();
        let middle = Vec3::new(1.0 / 3.0, 1.0 / 3.0, 0.0);
        // 6 N along +x acting 2 above the middle of the bottom face: the force, and 12 about y.
        let at = middle + Vec3::new(0.0, 0.0, 2.0);
        let push = Pushed {
            name: "RemoteForce1".into(),
            kind: LoadKind::RemoteForce { newtons: 6.0, direction: Vec3::X, point: at },
            faces: vec![0],
            edges: Vec::new(),
        };
        let p = build(mesh.clone(), &on, &setup(vec![fixed(&[3])], vec![push], surfaces.clone())).unwrap();
        assert!(near(p.total_force(), Vec3::new(6.0, 0.0, 0.0)), "{:?}", p.total_force());
        assert!(near(p.total_moment(middle), Vec3::new(0.0, 12.0, 0.0)), "{:?}", p.total_moment(middle));
        // About the point it acts at, it has no turning at all: it is that force, there.
        assert!(p.total_moment(at).len() < 1e-12, "{:?}", p.total_moment(at));
        // Acting at the middle itself it is an even push.
        let even = Pushed {
            name: "RemoteForce1".into(),
            kind: LoadKind::RemoteForce { newtons: 6.0, direction: Vec3::X, point: middle },
            faces: vec![0],
            edges: Vec::new(),
        };
        let p = build(mesh, &on, &setup(vec![fixed(&[3])], vec![even], surfaces)).unwrap();
        assert!(
            near(p.forces[&4], Vec3::new(2.0, 0.0, 0.0)) && p.forces[&0].len() < 1e-12,
            "a third of the 6 N on each mid-side node: {:?}",
            p.forces
        );
    }

    #[test]
    fn a_bearing_load_presses_on_the_half_of_a_round_face_that_it_pushes_against() {
        // A hole of radius 1 along z, as twelve flat-sided strips of one curved triangle pair
        // each would be much; here its wall as the spots of a mesh are: built by hand.
        let wall: Vec<Spot> = (0..360)
            .map(|deg| {
                let a = f64::from(deg).to_radians();
                let radial = Vec3::new(a.cos(), a.sin(), 0.0);
                // (Out of the solid is into the hole: toward its axis.)
                Spot {
                    at: radial,
                    da: radial * (-std::f64::consts::TAU / 360.0),
                    surface: Surface::Cylinder { origin: Vec3::ZERO, dir: Vec3::Z },
                    nodes: [0; 6],
                    weights: [0.0; 6],
                }
            })
            .collect();
        let pushes = bearing("Bearing1", &wall, 100.0, Vec3::new(3.0, 0.0, 0.0)).unwrap();
        let sum = pushes.iter().fold(Vec3::ZERO, |s, f| s + *f);
        assert!((sum - Vec3::new(100.0, 0.0, 0.0)).len() < 1e-9, "{sum:?}");
        // Most where the wall squarely faces the load, nothing at the sides or behind.
        assert!(pushes[0].len() > pushes[45].len() && pushes[45].len() > pushes[80].len() && pushes[80].len() > 0.0);
        assert!(pushes[90].len() < 1e-12 && pushes[180].len() == 0.0 && pushes[270].len() < 1e-12);
        // Each push is square to the wall, into the material.
        assert!((pushes[45].normalized() - wall[45].at).len() < 1e-12);
        // A direction with some of it along the axis presses with what is across it; a negative
        // load presses the other half.
        let slanted = bearing("Bearing1", &wall, 100.0, Vec3::new(1.0, 0.0, 5.0)).unwrap();
        assert!((slanted.iter().fold(Vec3::ZERO, |s, f| s + *f) - Vec3::new(100.0, 0.0, 0.0)).len() < 1e-9);
        let back = bearing("Bearing1", &wall, -100.0, Vec3::X).unwrap();
        assert!((back.iter().fold(Vec3::ZERO, |s, f| s + *f) - Vec3::new(-100.0, 0.0, 0.0)).len() < 1e-9 && back[0].len() == 0.0);
        // Along the axis there is nothing to press on; a flat face is not a bearing's.
        assert!(bearing("Bearing1", &wall, 100.0, Vec3::Z).unwrap_err().contains("across the axis"));
        let flat = [Spot { at: Vec3::ZERO, da: Vec3::Z, surface: Surface::Plane { normal: Vec3::Z }, nodes: [0; 6], weights: [0.0; 6] }];
        assert!(bearing("Bearing1", &flat, 100.0, Vec3::X).unwrap_err().contains("round (cylindrical) faces"));
    }

    /// The wall of a bore of radius 1 along z with `sides` flat sides, each with `spots` spots
    /// along it, as a shape's mesh has a small one, turned by `turn` degrees.
    fn flat_sided_bore(sides: u32, spots: u32, turn: f64) -> Vec<Spot> {
        let corner = |i: u32| {
            let a = f64::from(i) * std::f64::consts::TAU / f64::from(sides) + turn.to_radians();
            Vec3::new(a.cos(), a.sin(), 0.0)
        };
        (0..sides)
            .flat_map(|i| {
                let (a, b) = (corner(i), corner(i + 1));
                // (Out of the solid is into the hole: square to the side, toward the axis.)
                let square = ((a + b) * 0.5).normalized() * -1.0;
                (0..spots).map(move |k| Spot {
                    at: a + (b - a) * ((f64::from(k) + 0.5) / f64::from(spots)),
                    da: square * (a.dist(b) / f64::from(spots)),
                    surface: Surface::Cylinder { origin: Vec3::ZERO, dir: Vec3::Z },
                    nodes: [0; 6],
                    weights: [0.0; 6],
                })
            })
            .collect()
    }

    #[test]
    fn a_bearing_load_on_a_bore_of_few_flat_sides_goes_the_way_asked() {
        // From a triangle up, and every way round: all of it along the load, nothing across it
        // and nothing along the axis.
        let d = Vec3::new(1.0, 1.0, 0.0).normalized();
        for sides in [3, 4, 5, 6, 7, 12] {
            for turn in [0.0, 10.0, 17.0, 33.0, 45.0, 60.0, 90.0] {
                for spots in [1, 3, 6] {
                    let wall = flat_sided_bore(sides, spots, turn);
                    let pushes = bearing("Bearing1", &wall, 100.0, d).unwrap();
                    let sum = pushes.iter().fold(Vec3::ZERO, |s, f| s + *f);
                    assert!((sum - d * 100.0).len() < 1e-9, "{sides} sides, turned {turn}, {spots} spots: {sum:?}");
                    assert!(pushes.iter().all(|p| p.z == 0.0));
                }
            }
        }
        // Each push goes away from the axis at its spot on a bore: into the material.
        let wall = flat_sided_bore(12, 3, 0.0);
        let pushes = bearing("Bearing1", &wall, 100.0, Vec3::X).unwrap();
        assert!(wall.iter().zip(&pushes).all(|(s, p)| p.dot(s.at) >= -1e-9));
        // And toward it on a boss, where out of the solid is the other way.
        let boss: Vec<Spot> = wall.iter().map(|s| Spot { at: s.at, da: s.da * -1.0, surface: s.surface, nodes: [0; 6], weights: [0.0; 6] }).collect();
        let pushes = bearing("Bearing1", &boss, 100.0, Vec3::X).unwrap();
        assert!(boss.iter().zip(&pushes).all(|(s, p)| p.dot(s.at) <= 1e-9));
        assert!((pushes.iter().fold(Vec3::ZERO, |s, f| s + *f) - Vec3::X * 100.0).len() < 1e-9);
    }

    /// Two tetrahedra joined by nothing: nodes 0 to 9 one, 10 to 19 the other, a long way off.
    fn two_pieces() -> TetMesh {
        let mut mesh = read_msh(ONE_TET).unwrap();
        let (nodes, tet) = (mesh.nodes.clone(), mesh.tets[0]);
        mesh.nodes.extend(nodes.iter().map(|p| *p + Vec3::new(10.0, 0.0, 0.0)));
        mesh.tets.push(tet.map(|n| n + nodes.len() as u32));
        mesh
    }

    #[test]
    fn a_piece_that_is_not_held_is_found() {
        let problem = |mesh: TetMesh, held: &[u32]| Problem {
            mesh,
            material: steel(),
            held: held.iter().map(|n| (*n, vec![Vec3::X, Vec3::Y, Vec3::Z])).collect(),
            forces: BTreeMap::new(),
            gravity: None,
            spin: None,
        };
        let loose = |pieces, loose| Loose { pieces, loose };
        // One piece: held fast, not held, or held at one node (it can turn).
        let one = read_msh(ONE_TET).unwrap();
        assert_eq!(problem(one.clone(), &[0, 1, 2]).loose_pieces(), loose(1, 0));
        assert_eq!(problem(one.clone(), &[]).loose_pieces(), loose(1, 1));
        assert_eq!(problem(one, &[0]).loose_pieces(), loose(1, 1));
        // Two: the first held fast passes the check on the whole, and the second floats.
        let held_first = problem(two_pieces(), &[0, 1, 2]);
        assert_eq!(held_first.free_motions(), 0);
        assert_eq!(held_first.loose_pieces(), loose(2, 1));
        // Each held fast: none is loose. One held at a node and one at two: each can turn, which
        // the whole does not see.
        assert_eq!(problem(two_pieces(), &[0, 1, 2, 10, 11, 12]).loose_pieces(), loose(2, 0));
        let barely = problem(two_pieces(), &[0, 10, 12]);
        assert_eq!(barely.free_motions(), 0);
        assert_eq!(barely.loose_pieces(), loose(2, 2));
    }

    #[test]
    fn a_parts_turning_is_a_speed_about_an_axis_for_the_solver() {
        let (mesh, on, surfaces) = tet();
        let turning = Pushed {
            name: "Body1".into(),
            kind: LoadKind::Body { axis: Vec3::new(0.0, 0.0, 4.0), point: Vec3::new(1.0, 2.0, 3.0), rpm: 600.0 },
            faces: vec![],
            edges: Vec::new(),
        };
        let p = build(mesh, &on, &setup(vec![fixed(&[0])], vec![turning], surfaces)).unwrap();
        let spin = p.spin.expect("a spin");
        assert_eq!((spin.point, spin.axis), (Vec3::new(1.0, 2.0, 3.0), Vec3::Z));
        assert!((spin.omega - 20.0 * std::f64::consts::PI).abs() < 1e-12, "600 turns a minute is 20 pi radians a second: {}", spin.omega);
        assert!(p.forces.is_empty() && p.gravity.is_none());
        // (Three equations with one answer, and three with none.)
        let x = solve3([[2.0, 0.0, 0.0], [0.0, 4.0, 0.0], [1.0, 0.0, 5.0]], Vec3::new(2.0, 8.0, 11.0)).unwrap();
        assert!(near(x, Vec3::new(1.0, 2.0, 2.0)), "{x:?}");
        assert!(solve3([[1.0, 2.0, 3.0], [2.0, 4.0, 6.0], [0.0, 0.0, 1.0]], Vec3::X).is_none());
    }
}
