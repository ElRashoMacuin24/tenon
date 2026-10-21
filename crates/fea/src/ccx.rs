//! A problem written for CalculiX and its answer read back.
//!
//! CalculiX is a program of its own (GPL), started as one and never linked to: Tenon writes
//! the text it reads (`.inp`) and reads the text it writes (`.frd`). Both formats are
//! documented by its authors; the writer and the reader here are our own. Another solver
//! would be another module beside this one: nothing else in the crate knows of CalculiX.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use tenon_geom::Vec3;

use crate::problem::Problem;
use crate::topo::{LEVELS, modulus};

/// The heading of the energy each element holds in a `.dat` file (`ELSE`).
pub const ENERGY: &str = "internal energy (element, energy)";
/// The heading of each element's volume in a `.dat` file (`EVOL`).
pub const VOLUME: &str = "volume (element, volume)";

/// What a solver found: how far each node moved (mm), and the stress there (MPa: xx, yy, zz,
/// xy, yz, zx), node by node as the mesh numbers them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Solution {
    pub displacement: Vec<Vec3>,
    pub stress: Vec<[f64; 6]>,
}

/// A number as the solver reads it. It takes a number from at most twenty characters, so a
/// full sixteen digits with a sign and an exponent would be cut short and read wrong: twelve
/// digits after the point are written, and zeros at the end left off.
fn num(v: f64) -> String {
    let text = format!("{v:.12e}");
    match text.split_once('e') {
        Some((mantissa, exponent)) if mantissa.contains('.') => format!("{}e{exponent}", mantissa.trim_end_matches('0').trim_end_matches('.')),
        _ => text,
    }
}

/// The held directions of one node as conditions with one degree of freedom each to itself:
/// rows `a . u = 0` brought to the form where each has a leading 1 in a column no other row
/// uses. (A solver takes the leading one as the one that depends on the rest.)
fn conditions(dirs: &[Vec3]) -> Vec<[f64; 3]> {
    let mut rows: Vec<[f64; 3]> = dirs.iter().map(|d| [d.x, d.y, d.z]).collect();
    let mut done = 0;
    for col in 0..3 {
        // The row, among those not yet used, with the most of this column.
        let Some(best) = (done..rows.len()).max_by(|a, b| rows[*a][col].abs().total_cmp(&rows[*b][col].abs())) else { break };
        if rows[best][col].abs() < 1e-6 {
            continue;
        }
        rows.swap(done, best);
        let lead = rows[done][col];
        rows[done] = rows[done].map(|v| v / lead);
        for r in 0..rows.len() {
            if r != done {
                let k = rows[r][col];
                let pivot = rows[done];
                (0..3).for_each(|c| rows[r][c] -= k * pivot[c]);
            }
        }
        done += 1;
    }
    rows.truncate(done);
    // (What is left of rounding is nothing.)
    for row in &mut rows {
        row.iter_mut().for_each(|v| {
            if v.abs() < 1e-9 {
                *v = 0.0;
            }
        });
    }
    rows
}

/// The parts of a problem as CalculiX reads it that do not depend on the material: the mesh
/// (nodes and ten-node tetrahedra), the held directions that are equations between a node's
/// own movements, and what goes inside the step (the held directions along the axes and the
/// forces on nodes). A stress check puts one material between them; a shape being worked out
/// puts a material for each stiffness level in use, round after round, on the same pieces.
#[derive(Clone, Debug, PartialEq)]
pub struct Deck {
    /// `*NODE` and `*ELEMENT`.
    mesh: String,
    /// The `*EQUATION`s, which go before the step.
    equations: String,
    /// `*BOUNDARY` and `*CLOAD`, inside the step.
    step: String,
    /// How many elements the mesh has.
    elements: usize,
}

impl Deck {
    /// The pieces of `p`, its weight and turning left out.
    fn of(p: &Problem) -> Deck {
        let mut mesh = String::with_capacity(p.mesh.nodes.len() * 48 + p.mesh.tets.len() * 80);
        mesh.push_str("*NODE, NSET=NALL\n");
        for (i, n) in p.mesh.nodes.iter().enumerate() {
            let _ = writeln!(mesh, "{}, {}, {}, {}", i + 1, num(n.x), num(n.y), num(n.z));
        }
        mesh.push_str("*ELEMENT, TYPE=C3D10, ELSET=EALL\n");
        for (i, t) in p.mesh.tets.iter().enumerate() {
            // (The mesher has the last two mid-side nodes the other way round.)
            let order = [0, 1, 2, 3, 4, 5, 6, 7, 9, 8];
            let nodes: Vec<String> = order.iter().map(|k| (t[*k] + 1).to_string()).collect();
            let _ = writeln!(mesh, "{}, {}", i + 1, nodes.join(", "));
        }

        // Held directions: along one axis of the part, a boundary condition; along another
        // direction, an equation between the node's own movements.
        let (mut equations, mut boundary) = (String::new(), String::new());
        for (node, dirs) in &p.held {
            for row in conditions(dirs) {
                let terms: Vec<(usize, f64)> = row.iter().enumerate().filter(|(_, v)| **v != 0.0).map(|(i, v)| (i + 1, *v)).collect();
                match terms.as_slice() {
                    [] => {}
                    [(dof, _)] => {
                        let _ = writeln!(boundary, "{}, {dof}, {dof}", node + 1);
                    }
                    terms => {
                        let said: Vec<String> = terms.iter().map(|(dof, v)| format!("{}, {dof}, {}", node + 1, num(*v))).collect();
                        let _ = writeln!(equations, "*EQUATION\n{}\n{}", terms.len(), said.join(", "));
                    }
                }
            }
        }
        let mut step = String::new();
        if !boundary.is_empty() {
            step.push_str("*BOUNDARY\n");
            step.push_str(&boundary);
        }
        if !p.forces.is_empty() {
            step.push_str("*CLOAD\n");
            for (node, f) in &p.forces {
                for (dof, v) in [(1, f.x), (2, f.y), (3, f.z)] {
                    if v != 0.0 {
                        let _ = writeln!(step, "{}, {dof}, {}", node + 1, num(v));
                    }
                }
            }
        }
        Deck { mesh, equations, step, elements: p.mesh.tets.len() }
    }

    /// The pieces of `p`, for a shape to be worked out on. A part under its own weight, or
    /// turning, is refused: those loads come from the part's mass, which changes as the shape
    /// does.
    pub fn new(p: &Problem) -> Result<Deck, String> {
        if p.gravity.is_some() {
            return Err("a shape is not worked out for a part under its own weight yet: that load depends on how much material is kept".into());
        }
        if p.spin.is_some() {
            return Err("a shape is not worked out for a part that turns yet: that load depends on how much material is kept".into());
        }
        Ok(Deck::of(p))
    }

    /// How many elements the mesh has.
    pub fn elements(&self) -> usize {
        self.elements
    }

    /// The problem as CalculiX reads it with each element at its own stiffness level
    /// (`levels`, one for each element in the mesh's order; see [`modulus`]) of a material of
    /// Young's modulus `e0` (MPa) and Poisson's ratio `nu`: a set of elements, a material and a
    /// section for each level in use, and a request for the energy each element holds (with
    /// `volumes`, its volume too) in the `.dat` file. Nothing else is asked for, and nothing
    /// that needs a density is in it.
    pub fn shape_input(&self, levels: &[u16], e0: f64, nu: f64, volumes: bool) -> String {
        let mut by_level: BTreeMap<u16, Vec<usize>> = BTreeMap::new();
        for (i, level) in levels.iter().enumerate() {
            by_level.entry((*level).min(LEVELS)).or_default().push(i + 1);
        }
        let mut s = String::with_capacity(self.mesh.len() + self.equations.len() + self.step.len() + levels.len() * 8 + by_level.len() * 160);
        s.push_str("** Tenon Optimize Shape (mm, N, MPa, t): stiffness by element set\n");
        s.push_str(&self.mesh);
        for (level, ids) in &by_level {
            let _ = writeln!(s, "*ELSET, ELSET=L{level}");
            // (The solver reads at most sixteen to a line.)
            for row in ids.chunks(16) {
                let row: Vec<String> = row.iter().map(ToString::to_string).collect();
                let _ = writeln!(s, "{}", row.join(", "));
            }
            let _ = writeln!(
                s,
                "*MATERIAL, NAME=L{level}\n*ELASTIC\n{}, {}\n*SOLID SECTION, ELSET=L{level}, MATERIAL=L{level}",
                num(modulus(*level, e0)),
                num(nu)
            );
        }
        s.push_str(&self.equations);
        s.push_str("*STEP\n*STATIC\n");
        s.push_str(&self.step);
        s.push_str(if volumes { "*EL PRINT, ELSET=EALL\nELSE, EVOL\n*END STEP\n" } else { "*EL PRINT, ELSET=EALL\nELSE\n*END STEP\n" });
        s
    }
}

/// The problem as CalculiX reads it: nodes, ten-node tetrahedra, one linear elastic
/// material, the held directions as boundary conditions and equations, the forces on nodes,
/// the part's weight, and a request for displacements and stresses at the nodes.
pub fn write_inp(p: &Problem) -> String {
    let deck = Deck::of(p);
    let mut s = String::with_capacity(deck.mesh.len() + deck.equations.len() + deck.step.len() + 512);
    s.push_str("** Written by Tenon for a stress check. Units: mm, N, MPa, tonne, s.\n");
    s.push_str(&deck.mesh);
    let _ = writeln!(
        s,
        "*MATERIAL, NAME=M\n*ELASTIC\n{}, {}\n*DENSITY\n{}",
        num(p.material.youngs_modulus),
        num(p.material.poisson),
        num(p.material.density)
    );
    s.push_str("*SOLID SECTION, ELSET=EALL, MATERIAL=M\n");
    s.push_str(&deck.equations);
    s.push_str("*STEP\n*STATIC\n");
    s.push_str(&deck.step);
    // Loads on all of the part, which the solver works out from its mass: its weight, and its
    // being flung outward as it turns (the square of its speed, a point on the axis, the axis).
    let mut whole = String::new();
    if let Some(g) = p.gravity {
        let size = g.len();
        if size > 0.0 {
            let _ = writeln!(whole, "EALL, GRAV, {}, {}, {}, {}", num(size), num(g.x / size), num(g.y / size), num(g.z / size));
        }
    }
    if let Some(spin) = p.spin {
        let (at, axis) = (spin.point, spin.axis);
        let said = [spin.omega * spin.omega, at.x, at.y, at.z, axis.x, axis.y, axis.z].map(num).join(", ");
        let _ = writeln!(whole, "EALL, CENTRIF, {said}");
    }
    if !whole.is_empty() {
        s.push_str("*DLOAD\n");
        s.push_str(&whole);
    }
    s.push_str("*NODE FILE\nU\n*EL FILE\nS\n*END STEP\n");
    s
}

/// Reads the displacements and stresses of the last step from the text of a `.frd` file, for a
/// mesh of `nodes` nodes (numbered from 1 in the file, as `write_inp` wrote them).
pub fn read_frd(text: &str, nodes: usize) -> Result<Solution, String> {
    // A block of results: a header line ` -4  NAME ...`, its components (` -5`), then a line
    // ` -1` for each node: the node's number in ten columns, its values in twelve each.
    let block = |name: &str, width: usize| -> Result<Vec<Vec<f64>>, String> {
        let mut out: Vec<Vec<f64>> = vec![Vec::new(); nodes];
        let mut found = false;
        let mut inside = false;
        for line in text.lines() {
            if let Some(head) = line.strip_prefix(" -4") {
                inside = head.split_whitespace().next() == Some(name);
                if inside {
                    // (A later step's block takes the place of an earlier one's.)
                    found = true;
                    out.iter_mut().for_each(Vec::clear);
                }
                continue;
            }
            if !inside {
                continue;
            }
            if line.starts_with(" -3") {
                inside = false;
                continue;
            }
            let Some(rest) = line.strip_prefix(" -1") else { continue };
            let (id, values) = rest.split_at_checked(10).ok_or_else(|| format!("the solver's results have a line that is cut short: `{line}`"))?;
            let id: usize = id.trim().parse().map_err(|_| format!("the solver's results have a line with no node: `{line}`"))?;
            let slot = out.get_mut(id.wrapping_sub(1)).ok_or_else(|| format!("the solver's results speak of node {id}, and the mesh has {nodes}"))?;
            let mut rest = values;
            let mut read = Vec::with_capacity(width);
            while read.len() < width {
                let (field, more) =
                    rest.split_at_checked(12).ok_or_else(|| format!("the solver's results have a line that is cut short: `{line}`"))?;
                let v: f64 = field.trim().parse().map_err(|_| format!("the solver's results have a value that is not a number: `{field}`"))?;
                if !v.is_finite() {
                    return Err("the solver's results are not numbers: the study has no one answer (is the part held fast?)".into());
                }
                read.push(v);
                rest = more;
            }
            *slot = read;
        }
        if !found {
            return Err(format!("the solver's results have no {name}: it did not finish"));
        }
        if let Some(missing) = out.iter().position(Vec::is_empty) {
            return Err(format!("the solver's results have no {name} for node {}", missing + 1));
        }
        Ok(out)
    };
    let displacement = block("DISP", 3)?.into_iter().map(|v| Vec3::new(v[0], v[1], v[2])).collect();
    let stress = block("STRESS", 6)?.into_iter().map(|v| [v[0], v[1], v[2], v[3], v[4], v[5]]).collect();
    Ok(Solution { displacement, stress })
}

/// Reads one value for each element from the text of a `.dat` file, for a mesh of `elements`
/// elements (numbered from 1 in the file, as the mesh orders them): the last block under
/// `heading` (such as [`ENERGY`] or [`VOLUME`]) for the set of all of them. Every element must
/// be there once, with a number. A value a hair below nothing (rounding) is read as nothing;
/// one further below is refused.
pub fn read_dat_elements(text: &str, heading: &str, elements: usize) -> Result<Vec<f64>, String> {
    // What the values are, in words: the heading up to its bracket.
    let what = heading.split(" (").next().unwrap_or(heading).trim();
    // The block is its heading, a blank line, then a line for each element: its number in ten
    // columns, two spaces, its value. (The solver writes a block each time it is asked: the
    // last is the answer. The energy density of each point of an element has a heading of its
    // own, which is not this one.)
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let start = lines
        .iter()
        .rposition(|l| l.starts_with(heading) && l.contains("for set EALL"))
        .ok_or_else(|| format!("the solver's results have no {what}: it did not finish"))?;
    let mut read: Vec<Option<f64>> = vec![None; elements];
    for line in lines[start + 1..].iter().skip_while(|l| l.is_empty()) {
        let mut fields = line.split_whitespace();
        let (Some(id), Some(value), None) = (fields.next(), fields.next(), fields.next()) else { break };
        let (Ok(id), Ok(value)) = (id.parse::<usize>(), value.parse::<f64>()) else { break };
        if !value.is_finite() {
            return Err(format!(
                "the solver's results are not numbers ({what} of element {id}): the study has no one answer (is the part held fast?)"
            ));
        }
        let slot =
            read.get_mut(id.wrapping_sub(1)).ok_or_else(|| format!("the solver's results speak of element {id}, and the mesh has {elements}"))?;
        if slot.is_some() {
            return Err(format!("the solver's results give the {what} of element {id} twice"));
        }
        *slot = Some(value);
    }
    let mut values = Vec::with_capacity(elements);
    for (i, v) in read.iter().enumerate() {
        values.push(v.ok_or_else(|| format!("the solver's results have no {what} for element {}", i + 1))?);
    }
    let most = values.iter().fold(0.0f64, |m, v| m.max(*v));
    for (i, v) in values.iter_mut().enumerate() {
        if v.is_sign_negative() {
            if *v < -1e-12 * most {
                return Err(format!(
                    "the solver's results say the {what} of element {} is {v}, less than nothing: its answer cannot be right",
                    i + 1
                ));
            }
            *v = 0.0;
        }
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::read_msh;
    use crate::mesh::tests::ONE_TET;
    use crate::problem::Material;
    use std::collections::BTreeMap;

    fn tet() -> Problem {
        Problem {
            mesh: read_msh(ONE_TET).unwrap(),
            material: Material::of(200.0, 0.29, 7.85),
            held: BTreeMap::new(),
            forces: BTreeMap::new(),
            gravity: None,
            spin: None,
        }
    }

    #[test]
    fn held_directions_become_conditions_each_with_a_freedom_of_its_own() {
        let near = |a: &[f64; 3], b: [f64; 3]| (0..3).all(|i| (a[i] - b[i]).abs() < 1e-12);
        // Along the axes: themselves.
        assert_eq!(conditions(&[Vec3::X, Vec3::Y, Vec3::Z]), [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
        assert_eq!(conditions(&[-Vec3::Z]), [[0.0, 0.0, 1.0]]);
        // One slanted direction: its largest part leads.
        let c = conditions(&[Vec3::new(0.6, 0.0, -0.8)]);
        assert!(matches!(c.as_slice(), [row] if near(row, [1.0, 0.0, -0.8 / 0.6])), "{c:?}");
        // Two slanted directions whose largest parts are both along x: each row still leads
        // with a column the other does not use.
        let (a, b) = (Vec3::new(1.0, 0.0, 0.1).normalized(), Vec3::new(1.0, 0.1, 0.0).normalized());
        let b = (b - a * b.dot(a)).normalized();
        let c = conditions(&[a, b]);
        assert_eq!(c.len(), 2);
        assert!(near(&c[0], [1.0, 0.0, 0.1]) || (c[0][0] == 1.0 && c[0][1] == 0.0), "{c:?}");
        assert!(c[1][0] == 0.0 && c[1][1] == 1.0, "{c:?}");
        // Both conditions still say what the directions said: each is square to the free way.
        let free = a.cross(b);
        for row in &c {
            assert!((row[0] * free.x + row[1] * free.y + row[2] * free.z).abs() < 1e-9, "{c:?}");
        }
        assert!(conditions(&[]).is_empty());
    }

    #[test]
    fn a_problem_is_written_as_the_solver_reads_it() {
        let mut p = tet();
        p.held.insert(0, vec![Vec3::X, Vec3::Y, Vec3::Z]);
        p.held.insert(1, vec![Vec3::new(0.6, 0.0, 0.8)]);
        p.held.insert(2, vec![-Vec3::Z]);
        p.forces.insert(3, Vec3::new(0.0, 2.5, -10.0));
        p.gravity = Some(Vec3::new(0.0, 0.0, -9810.0));
        let text = write_inp(&p);
        let has = |what: &str| assert!(text.contains(what), "`{what}` is not in:\n{text}");
        has("*NODE, NSET=NALL\n1, 0e0, 0e0, 0e0\n2, 1e0, 0e0, 0e0\n");
        // The tetrahedron with its last two mid-side nodes exchanged.
        has("*ELEMENT, TYPE=C3D10, ELSET=EALL\n1, 1, 2, 3, 4, 5, 6, 7, 8, 10, 9\n");
        has("*MATERIAL, NAME=M\n*ELASTIC\n2e5, 2.9e-1\n*DENSITY\n7.85e-9\n*SOLID SECTION, ELSET=EALL, MATERIAL=M\n");
        // A slanted direction is an equation between the node's own movements, before the step.
        has("*EQUATION\n2\n2, 1, 1e0, 2, 3, 1.333333333333e0\n*STEP\n*STATIC\n");
        // No number is longer than the twenty characters the solver reads.
        assert_eq!(num(-1.0 / 3.0e-100), "-3.333333333333e99");
        assert_eq!((num(0.0), num(-0.5), num(123_456.789_012_345)), ("0e0".to_owned(), "-5e-1".to_owned(), "1.234567890123e5".to_owned()));
        assert!([f64::MAX, f64::MIN_POSITIVE, -1.234_567_890_123_456e-300].iter().all(|v| num(*v).len() <= 20));
        // Directions along the axes are boundary conditions.
        has("*BOUNDARY\n1, 1, 1\n1, 2, 2\n1, 3, 3\n3, 3, 3\n");
        has("*CLOAD\n4, 2, 2.5e0\n4, 3, -1e1\n");
        has("*DLOAD\nEALL, GRAV, 9.81e3, 0e0, 0e0, -1e0\n*NODE FILE");
        // Turning at 10 radians a second about z through (1, 2, 0), with its weight: both.
        p.spin = Some(crate::problem::Spin { point: Vec3::new(1.0, 2.0, 0.0), axis: Vec3::Z, omega: 10.0 });
        let text = write_inp(&p);
        assert!(text.contains("*DLOAD\nEALL, GRAV, 9.81e3, 0e0, 0e0, -1e0\nEALL, CENTRIF, 1e2, 1e0, 2e0, 0e0, 0e0, 0e0, 1e0\n*NODE FILE"), "{text}");
        p.gravity = None;
        assert!(write_inp(&p).contains("*DLOAD\nEALL, CENTRIF, 1e2"));
        assert!(text.ends_with("*NODE FILE\nU\n*EL FILE\nS\n*END STEP\n"));
        // With nothing held or pushed, those sections are not written at all.
        let bare = write_inp(&tet());
        assert!(!bare.contains("*BOUNDARY") && !bare.contains("*CLOAD") && !bare.contains("*DLOAD") && !bare.contains("*EQUATION"), "{bare}");
    }

    /// The tetrahedron held, pushed, weighed and turned: something in every part of the input.
    fn everything() -> Problem {
        let mut p = tet();
        p.held.insert(0, vec![Vec3::X, Vec3::Y, Vec3::Z]);
        p.held.insert(1, vec![Vec3::new(0.6, 0.0, 0.8)]);
        p.held.insert(2, vec![-Vec3::Z]);
        p.held.insert(5, vec![Vec3::new(1.0, 1.0, 0.0).normalized(), Vec3::Z]);
        p.forces.insert(3, Vec3::new(0.0, 2.5, -10.0));
        p.forces.insert(8, Vec3::new(-1.0 / 3.0, 0.0, 0.0));
        p.gravity = Some(Vec3::new(0.0, 0.0, -9810.0));
        p.spin = Some(crate::problem::Spin { point: Vec3::new(1.0, 2.0, 0.0), axis: Vec3::Z, omega: 10.0 });
        p
    }

    #[test]
    fn a_stress_input_is_written_as_before() {
        // (Written by the stress check as it was before its pieces were shared with shapes.)
        let want = concat!(
            "** Written by Tenon for a stress check. Units: mm, N, MPa, tonne, s.\n",
            "*NODE, NSET=NALL\n",
            "1, 0e0, 0e0, 0e0\n",
            "2, 1e0, 0e0, 0e0\n",
            "3, 0e0, 1e0, 0e0\n",
            "4, 0e0, 0e0, 1e0\n",
            "5, 5e-1, 0e0, 0e0\n",
            "6, 5e-1, 5e-1, 0e0\n",
            "7, 0e0, 5e-1, 0e0\n",
            "8, 0e0, 0e0, 5e-1\n",
            "9, 0e0, 5e-1, 5e-1\n",
            "10, 5e-1, 0e0, 5e-1\n",
            "*ELEMENT, TYPE=C3D10, ELSET=EALL\n",
            "1, 1, 2, 3, 4, 5, 6, 7, 8, 10, 9\n",
            "*MATERIAL, NAME=M\n",
            "*ELASTIC\n",
            "2e5, 2.9e-1\n",
            "*DENSITY\n",
            "7.85e-9\n",
            "*SOLID SECTION, ELSET=EALL, MATERIAL=M\n",
            "*EQUATION\n",
            "2\n",
            "2, 1, 1e0, 2, 3, 1.333333333333e0\n",
            "*EQUATION\n",
            "2\n",
            "6, 1, 1e0, 6, 2, 1e0\n",
            "*STEP\n",
            "*STATIC\n",
            "*BOUNDARY\n",
            "1, 1, 1\n",
            "1, 2, 2\n",
            "1, 3, 3\n",
            "3, 3, 3\n",
            "6, 3, 3\n",
            "*CLOAD\n",
            "4, 2, 2.5e0\n",
            "4, 3, -1e1\n",
            "9, 1, -3.333333333333e-1\n",
            "*DLOAD\n",
            "EALL, GRAV, 9.81e3, 0e0, 0e0, -1e0\n",
            "EALL, CENTRIF, 1e2, 1e0, 2e0, 0e0, 0e0, 0e0, 1e0\n",
            "*NODE FILE\n",
            "U\n",
            "*EL FILE\n",
            "S\n",
            "*END STEP\n",
        );
        assert_eq!(write_inp(&everything()), want);
        // With nothing held or pushed: the mesh, the material and the request alone.
        let bare = want.split("*EQUATION").next().unwrap().to_owned() + "*STEP\n*STATIC\n*NODE FILE\nU\n*EL FILE\nS\n*END STEP\n";
        assert_eq!(write_inp(&tet()), bare);
    }

    /// The text of `text` from the start of `from` to the start of `to` after it.
    fn between<'a>(text: &'a str, from: &str, to: &str) -> &'a str {
        let start = text.find(from).unwrap_or_else(|| panic!("no `{from}` in:\n{text}"));
        let end = text[start..].find(to).unwrap_or_else(|| panic!("no `{to}` after `{from}` in:\n{text}"));
        &text[start..start + end]
    }

    /// A block of 48 elements held at x = 0 (and one node there only along a slant), pushed at
    /// its far corner.
    fn block() -> Problem {
        let mesh = crate::mesh::TetMesh::block([2, 2, 2], Vec3::new(20.0, 10.0, 10.0));
        let mut held = BTreeMap::new();
        for (i, n) in mesh.nodes.iter().enumerate() {
            if n.x == 0.0 {
                held.insert(i as u32, vec![Vec3::X, Vec3::Y, Vec3::Z]);
            }
        }
        let slanted = mesh.nodes.iter().position(|n| n.x == 0.0).unwrap() as u32;
        held.insert(slanted, vec![Vec3::new(0.0, 0.6, 0.8)]);
        let corner = mesh.nodes.iter().position(|n| *n == Vec3::new(20.0, 10.0, 10.0)).unwrap() as u32;
        let forces = BTreeMap::from([(corner, Vec3::new(0.0, 0.0, -100.0))]);
        Problem { mesh, material: Material::of(200.0, 0.3, 7.85), held, forces, gravity: None, spin: None }
    }

    #[test]
    fn a_shape_input_has_a_set_a_material_and_a_section_for_each_level_used() {
        let p = block();
        let deck = Deck::new(&p).unwrap();
        assert_eq!(deck.elements(), 48);
        // Every third element solid, the rest at the floor but two near half; one beyond
        // solid is solid.
        let mut levels: Vec<u16> = (0..48).map(|i| if i % 3 == 2 { 1000 } else { 0 }).collect();
        (levels[4], levels[7], levels[47]) = (499, 500, 1200);
        let e0 = 200_000.0;
        let text = deck.shape_input(&levels, e0, 0.3, false);
        assert!(
            text.starts_with("** Tenon Optimize Shape (mm, N, MPa, t): stiffness by element set\n*NODE, NSET=NALL\n1, 0e0, 0e0, 0e0\n"),
            "{text}"
        );
        // The mesh, the equations and the step are the stress check's own.
        let stress = write_inp(&p);
        assert_eq!(between(&text, "*NODE", "*ELSET"), between(&stress, "*NODE", "*MATERIAL"));
        assert!(text.contains(between(&stress, "*EQUATION", "*STEP")) && text.contains("*EQUATION\n2\n"), "{text}");
        assert!(text.contains(&(between(&stress, "*STEP", "*NODE FILE").to_owned() + "*EL PRINT")), "{text}");
        // A set, a material and a section for each level used, from the least.
        let at = |what: &str| text.find(what).unwrap_or_else(|| panic!("no `{what}` in:\n{text}"));
        assert!(at("*ELSET, ELSET=L0\n") < at("*ELSET, ELSET=L499\n") && at("*ELSET, ELSET=L499\n") < at("*ELSET, ELSET=L500\n"));
        assert!(at("*ELSET, ELSET=L500\n") < at("*ELSET, ELSET=L1000\n") && at("*ELSET, ELSET=L1000\n") < at("*EQUATION"));
        assert_eq!(text.matches("*MATERIAL").count(), 4, "{text}");
        assert!(!text.contains("L1200"), "{text}");
        for level in [0u16, 499, 500, 1000] {
            let block =
                format!("*MATERIAL, NAME=L{level}\n*ELASTIC\n{}, 3e-1\n*SOLID SECTION, ELSET=L{level}, MATERIAL=L{level}\n", num(modulus(level, e0)));
            assert!(text.contains(&block), "no `{block}` in:\n{text}");
        }
        assert!(text.contains("*ELASTIC\n2e5, 3e-1\n") && text.contains("*ELASTIC\n2e-4, 3e-1\n"), "solid, and the floor: {text}");
        assert_eq!(num(modulus(500, e0)), "2.5000000175e4");
        // Each element in exactly one set, at its own level, the numbers rising and at most
        // sixteen to a line.
        let mut set_of: BTreeMap<usize, u16> = BTreeMap::new();
        let mut lines = text.lines().peekable();
        while let Some(line) = lines.next() {
            let Some(level) = line.strip_prefix("*ELSET, ELSET=L") else { continue };
            let level: u16 = level.parse().unwrap();
            let mut last = 0;
            while let Some(row) = lines.next_if(|l| !l.starts_with('*')) {
                let ids: Vec<usize> = row.split(", ").map(|v| v.parse().unwrap()).collect();
                assert!(!ids.is_empty() && ids.len() <= 16 && !row.ends_with(','), "`{row}`");
                for id in ids {
                    assert!(id > last, "`{row}`");
                    last = id;
                    assert_eq!(set_of.insert(id, level), None, "element {id} in two sets");
                }
            }
        }
        let want: BTreeMap<usize, u16> = levels.iter().enumerate().map(|(i, l)| (i + 1, (*l).min(1000))).collect();
        assert_eq!(set_of, want);
        assert!(
            text.contains(concat!(
                "*ELSET, ELSET=L0\n1, 2, 4, 7, 10, 11, 13, 14, 16, 17, 19, 20, 22, 23, 25, 26\n",
                "28, 29, 31, 32, 34, 35, 37, 38, 40, 41, 43, 44, 46, 47\n*MATERIAL, NAME=L0\n"
            )),
            "sixteen to a line: {text}"
        );
        assert!(text.contains("*ELSET, ELSET=L1000\n3, 6, 9, 12, 15, 18, 21, 24, 27, 30, 33, 36, 39, 42, 45, 48\n*MATERIAL"), "{text}");
        assert!(text.contains("*ELSET, ELSET=L499\n5\n*MATERIAL") && text.contains("*ELSET, ELSET=L500\n8\n*MATERIAL"), "{text}");
        // Nothing asked for but the energies; nothing that needs a density.
        for not in ["*NODE FILE", "*EL FILE", "*DENSITY", "*DLOAD", "*NODE PRINT"] {
            assert!(!text.contains(not), "`{not}` in:\n{text}");
        }
        assert!(text.ends_with("\n*EL PRINT, ELSET=EALL\nELSE\n*END STEP\n"), "{text}");
        // The volumes too, when asked.
        assert!(deck.shape_input(&levels, e0, 0.3, true).ends_with("\n*EL PRINT, ELSET=EALL\nELSE, EVOL\n*END STEP\n"));
        // All at one level: one of each.
        let solid = deck.shape_input(&[1000; 48], e0, 0.3, false);
        assert_eq!((solid.matches("*ELSET").count(), solid.matches("*MATERIAL").count(), solid.matches("*SOLID SECTION").count()), (1, 1, 1));
    }

    #[test]
    fn a_shape_input_refuses_weight_and_turning() {
        let mut p = everything();
        let e = Deck::new(&p).unwrap_err();
        assert!(e.contains("under its own weight") && e.contains("depends on how much material is kept"), "{e}");
        p.gravity = None;
        let e = Deck::new(&p).unwrap_err();
        assert!(e.contains("a part that turns") && e.contains("depends on how much material is kept"), "{e}");
        p.spin = None;
        let deck = Deck::new(&p).unwrap();
        assert_eq!(deck.elements(), 1);
        // Without them a stress check's input has no whole-part loads either.
        assert!(!write_inp(&p).contains("*DLOAD"));
    }

    /// A `.dat` file as the solver writes it (lines ending in CR LF), with two increments:
    /// each the energies and the volumes of three elements, then the energy density at each
    /// point of each element, and a block for another set.
    const DAT: &str = concat!(
        "\r\n",
        "                        S T E P       1\r\n",
        "\r\n",
        "\r\n",
        "                                INCREMENT     1\r\n",
        "\r\n",
        "\r\n",
        " internal energy (element, energy) for set EALL and time  0.5000000E+00\r\n",
        "\r\n",
        "         1  1.000000E+00\r\n",
        "         2  2.000000E+00\r\n",
        "         3  3.000000E+00\r\n",
        "\r\n",
        " volume (element, volume) for set EALL and time  0.5000000E+00\r\n",
        "\r\n",
        "         1  1.666667E-01\r\n",
        "         2  5.000000E-01\r\n",
        "         3  2.500000E-01\r\n",
        "\r\n",
        "                                INCREMENT     2\r\n",
        "\r\n",
        "\r\n",
        " internal energy (element, energy) for set EALL and time  0.1000000E+01\r\n",
        "\r\n",
        "         2  4.000000E+00\r\n",
        "         1  2.198213E-05\r\n",
        "         3  0.000000E+00\r\n",
        "\r\n",
        " volume (element, volume) for set EALL and time  0.1000000E+01\r\n",
        "\r\n",
        "         1  1.666667E-01\r\n",
        "         2  5.000000E-01\r\n",
        "         3  2.500000E-01\r\n",
        "\r\n",
        " internal energy density (elem, integ.pnt.,energy) for set EALL and time  0.1000000E+01\r\n",
        "\r\n",
        "         1   1  5.847918E-01\r\n",
        "         1   2  9.574096E-02\r\n",
        "         2   1  7.000000E+00\r\n",
        "         3   1  8.000000E+00\r\n",
        "\r\n",
        " internal energy (element, energy) for set L500 and time  0.1000000E+01\r\n",
        "\r\n",
        "         1  9.000000E+00\r\n",
    );

    #[test]
    fn element_energies_are_read_from_the_last_block() {
        assert!(DAT.contains("\r\n"));
        assert_eq!(read_dat_elements(DAT, ENERGY, 3).unwrap(), [2.198213e-5, 4.0, 0.0]);
        assert_eq!(read_dat_elements(DAT, VOLUME, 3).unwrap(), [1.666667e-1, 0.5, 0.25]);
        // With plain line ends, the same; the block may end the file.
        let plain = DAT.replace("\r\n", "\n");
        assert_eq!(read_dat_elements(&plain, ENERGY, 3).unwrap(), [2.198213e-5, 4.0, 0.0]);
        let ends = &plain[..plain.find(" volume (element, volume) for set EALL and time  0.1").unwrap()];
        assert_eq!(read_dat_elements(ends.trim_end(), ENERGY, 3).unwrap(), [2.198213e-5, 4.0, 0.0]);
        // Only the first increment: its values.
        let first = &DAT[..DAT.find("INCREMENT     2").unwrap()];
        assert_eq!(read_dat_elements(first, ENERGY, 3).unwrap(), [1.0, 2.0, 3.0]);
    }

    #[test]
    fn the_energy_density_block_is_not_taken() {
        // The density at each point comes after the energies, and is not them.
        let energies = read_dat_elements(DAT, ENERGY, 3).unwrap();
        assert!(!energies.contains(&7.0) && !energies.contains(&0.5847918), "{energies:?}");
        // With the energies themselves not there, there are none to read.
        let only_density = &DAT[DAT.find(" internal energy density").unwrap()..DAT.find(" internal energy (element, energy) for set L500").unwrap()];
        let e = read_dat_elements(only_density, ENERGY, 3).unwrap_err();
        assert_eq!(e, "the solver's results have no internal energy: it did not finish");
        // Nor are another set's.
        let other_set = &DAT[DAT.find(" internal energy (element, energy) for set L500").unwrap()..];
        assert_eq!(read_dat_elements(other_set, ENERGY, 1).unwrap_err(), "the solver's results have no internal energy: it did not finish");
        assert_eq!(read_dat_elements("", VOLUME, 3).unwrap_err(), "the solver's results have no volume: it did not finish");
    }

    #[test]
    fn a_missing_or_unknown_element_or_a_nan_is_refused_in_words() {
        let told = |text: &str, elements: usize, what: &str| {
            let e = read_dat_elements(text, ENERGY, elements).unwrap_err();
            assert!(e.contains(what), "`{what}` is not in: {e}");
        };
        told(DAT, 4, "have no internal energy for element 4");
        told(&DAT.replace("         1  2.198213E-05\r\n", ""), 3, "have no internal energy for element 1");
        told(DAT, 2, "speak of element 3, and the mesh has 2");
        told(&DAT.replace("         3  0.000000E+00", "         0  0.000000E+00"), 3, "speak of element 0, and the mesh has 3");
        told(&DAT.replace("2.198213E-05", "         NaN"), 3, "not numbers (internal energy of element 1): the study has no one answer");
        told(&DAT.replace("2.198213E-05", "Infinity"), 3, "not numbers");
        told(&DAT.replace("         3  0.000000E+00", "         2  0.000000E+00"), 3, "give the internal energy of element 2 twice");
        // A line that is not a value ends the block: what is after it is not read.
        told(&DAT.replace("         1  2.198213E-05", "   ***  ***"), 3, "have no internal energy for element 1");
        told("garbage", 3, "have no internal energy: it did not finish");
    }

    #[test]
    fn tiny_negative_energies_are_read_as_nothing() {
        let read = read_dat_elements(&DAT.replace("0.000000E+00\r\n\r\n volume", "-1.000000E-20\r\n\r\n volume"), ENERGY, 3).unwrap();
        assert_eq!(read, [2.198213e-5, 4.0, 0.0]);
        assert!(read[2].is_sign_positive(), "nothing, not less");
        // Far below nothing: refused.
        let e = read_dat_elements(&DAT.replace("0.000000E+00\r\n\r\n volume", "-1.000000E+00\r\n\r\n volume"), ENERGY, 3).unwrap_err();
        assert_eq!(e, "the solver's results say the internal energy of element 3 is -1, less than nothing: its answer cannot be right");
        // (A hair is a share of the most: beside 4, a millionth of a millionth of it.)
        let hair = read_dat_elements(&DAT.replace("0.000000E+00\r\n\r\n volume", "-3.900000E-12\r\n\r\n volume"), ENERGY, 3).unwrap();
        assert_eq!(hair[2], 0.0);
        assert!(read_dat_elements(&DAT.replace("0.000000E+00\r\n\r\n volume", "-4.100000E-12\r\n\r\n volume"), ENERGY, 3).is_err());
    }

    /// A results file of two nodes, as the solver lays one out.
    const FRD: &str = concat!(
        "    1C\n",
        "    2C                             2                                     1\n",
        " -1         1 0.00000E+00 0.00000E+00 0.00000E+00\n",
        " -3\n",
        "    1PSTEP                         1           1           1\n",
        "  100CL  101 1.000000000           2                     0    1           1\n",
        " -4  DISP        4    1\n",
        " -5  D1          1    2    1    0\n",
        " -5  D2          1    2    2    0\n",
        " -5  D3          1    2    3    0\n",
        " -5  ALL         1    2    0    0    1ALL\n",
        " -1         1 0.00000E+00 0.00000E+00 0.00000E+00\n",
        " -1         2 1.49404E-02 6.00777E-06-2.00067E-01\n",
        " -3\n",
        "    1PSTEP                         2           1           1\n",
        "  100CL  101 1.000000000           2                     0    1           1\n",
        " -4  STRESS      6    1\n",
        " -5  SXX         1    4    1    1\n",
        " -1         1 5.96542E+01-1.20000E+00 3.00000E-01 1.00000E+00-2.00000E+00 4.00000E+00\n",
        " -1         2-1.00000E+00 0.00000E+00 0.00000E+00 0.00000E+00 0.00000E+00 0.00000E+00\n",
        " -3\n",
        " 9999\n",
    );

    #[test]
    fn the_solvers_results_are_read_back() {
        let s = read_frd(FRD, 2).unwrap();
        assert_eq!(s.displacement, [Vec3::ZERO, Vec3::new(1.49404e-2, 6.00777e-6, -2.00067e-1)]);
        assert_eq!(s.stress[0], [59.6542, -1.2, 0.3, 1.0, -2.0, 4.0]);
        assert_eq!(s.stress[1][0], -1.0);
        // (The node coordinates at the top, which also start their lines with -1, are not results.)
        let told = |text: &str, nodes: usize, what: &str| {
            let e = read_frd(text, nodes).unwrap_err();
            assert!(e.contains(what), "{e}");
        };
        told(FRD, 3, "no DISP for node 3");
        told(FRD, 1, "speak of node 2, and the mesh has 1");
        told(&FRD.replace(" -4  STRESS", " -4  STRAIN"), 2, "no STRESS: it did not finish");
        told(&FRD.replace("-2.00067E-01", "         NaN"), 2, "no one answer");
        told(&FRD.replace("6.00777E-06-2.00067E-01", "6.00777E-06"), 2, "cut short");
        told(&FRD.replace(" 1.49404E-02", " ***********"), 2, "not a number");
        told("", 2, "no DISP");
    }
}
