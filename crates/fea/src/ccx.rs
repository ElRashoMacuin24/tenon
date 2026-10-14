//! A problem written for CalculiX and its answer read back.
//!
//! CalculiX is a program of its own (GPL), started as one and never linked to: Tenon writes
//! the text it reads (`.inp`) and reads the text it writes (`.frd`). Both formats are
//! documented by its authors; the writer and the reader here are our own. Another solver
//! would be another module beside this one: nothing else in the crate knows of CalculiX.

use std::fmt::Write as _;

use tenon_geom::Vec3;

use crate::problem::Problem;

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

/// The problem as CalculiX reads it: nodes, ten-node tetrahedra, one linear elastic
/// material, the held directions as boundary conditions and equations, the forces on nodes,
/// the part's weight, and a request for displacements and stresses at the nodes.
pub fn write_inp(p: &Problem) -> String {
    let mut s = String::with_capacity(p.mesh.nodes.len() * 48 + p.mesh.tets.len() * 80);
    s.push_str("** Written by Tenon for a stress check. Units: mm, N, MPa, tonne, s.\n*NODE, NSET=NALL\n");
    for (i, n) in p.mesh.nodes.iter().enumerate() {
        let _ = writeln!(s, "{}, {}, {}, {}", i + 1, num(n.x), num(n.y), num(n.z));
    }
    s.push_str("*ELEMENT, TYPE=C3D10, ELSET=EALL\n");
    for (i, t) in p.mesh.tets.iter().enumerate() {
        // (The mesher has the last two mid-side nodes the other way round.)
        let order = [0, 1, 2, 3, 4, 5, 6, 7, 9, 8];
        let nodes: Vec<String> = order.iter().map(|k| (t[*k] + 1).to_string()).collect();
        let _ = writeln!(s, "{}, {}", i + 1, nodes.join(", "));
    }
    let _ = writeln!(
        s,
        "*MATERIAL, NAME=M\n*ELASTIC\n{}, {}\n*DENSITY\n{}",
        num(p.material.youngs_modulus),
        num(p.material.poisson),
        num(p.material.density)
    );
    s.push_str("*SOLID SECTION, ELSET=EALL, MATERIAL=M\n");

    // Held directions: along one axis of the part, a boundary condition; along another
    // direction, an equation between the node's own movements.
    let mut boundary = String::new();
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
                    let _ = writeln!(s, "*EQUATION\n{}\n{}", terms.len(), said.join(", "));
                }
            }
        }
    }
    s.push_str("*STEP\n*STATIC\n");
    if !boundary.is_empty() {
        s.push_str("*BOUNDARY\n");
        s.push_str(&boundary);
    }
    if !p.forces.is_empty() {
        s.push_str("*CLOAD\n");
        for (node, f) in &p.forces {
            for (dof, v) in [(1, f.x), (2, f.y), (3, f.z)] {
                if v != 0.0 {
                    let _ = writeln!(s, "{}, {dof}, {}", node + 1, num(v));
                }
            }
        }
    }
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
