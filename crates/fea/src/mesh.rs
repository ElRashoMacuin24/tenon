//! The mesh a part is divided into to be solved: ten-node tetrahedra, and the six-node
//! triangles of its boundary, each saying which surface of the meshed solid it lies on.
//!
//! Read from the mesher's file (Gmsh's MSH format, version 2.2, as text). The format is
//! documented by its authors; this reader is our own.

use std::collections::HashMap;

use tenon_geom::Vec3;

/// The most nodes a mesh may have to be read (a hostile or runaway file is refused, not
/// loaded). A quick check is far below this.
pub const MAX_NODES: usize = 5_000_000;

/// A six-node triangle on the boundary: its three corners, then the nodes in the middle of its
/// sides (between corners 1 and 2, 2 and 3, 3 and 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tri6 {
    /// The surface of the meshed solid it lies on, as the mesher numbers them.
    pub surface: u32,
    pub nodes: [u32; 6],
}

/// A three-node piece of a curve of the meshed solid (an edge of the part): its two ends, then
/// the node in its middle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Line3 {
    /// The curve it is a piece of, as the mesher numbers them.
    pub curve: u32,
    pub nodes: [u32; 3],
}

/// A mesh of second-order tetrahedra with its boundary triangles. Nodes are numbered from 0.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TetMesh {
    pub nodes: Vec<Vec3>,
    /// Four corners, then the mid-side nodes between corners 1-2, 2-3, 3-1, 1-4, 3-4, 2-4 (the
    /// mesher's order).
    pub tets: Vec<[u32; 10]>,
    pub tris: Vec<Tri6>,
    /// The pieces of the solid's curves (its edges), for what is held or loaded along one.
    pub lines: Vec<Line3>,
}

/// The points and weights of a rule that integrates over a triangle (exact to the fourth
/// degree): (xi, eta, weight), the weights summing to 1.
const RULE: [(f64, f64, f64); 6] = {
    const A: f64 = 0.445_948_490_915_965;
    const B: f64 = 0.091_576_213_509_771;
    const WA: f64 = 0.223_381_589_678_011;
    const WB: f64 = 0.109_951_743_655_322;
    [(A, A, WA), (A, 1.0 - 2.0 * A, WA), (1.0 - 2.0 * A, A, WA), (B, B, WB), (B, 1.0 - 2.0 * B, WB), (1.0 - 2.0 * B, B, WB)]
};

/// The six shape functions of a second-order triangle at (xi, eta), and their derivatives
/// along xi and eta.
fn shape(xi: f64, eta: f64) -> ([f64; 6], [f64; 6], [f64; 6]) {
    let zeta = 1.0 - xi - eta;
    let n = [zeta * (2.0 * zeta - 1.0), xi * (2.0 * xi - 1.0), eta * (2.0 * eta - 1.0), 4.0 * xi * zeta, 4.0 * xi * eta, 4.0 * eta * zeta];
    let dxi = [-(4.0 * zeta - 1.0), 4.0 * xi - 1.0, 0.0, 4.0 * (zeta - xi), 4.0 * eta, -4.0 * eta];
    let deta = [-(4.0 * zeta - 1.0), 0.0, 4.0 * eta - 1.0, -4.0 * xi, 4.0 * xi, 4.0 * (zeta - eta)];
    (n, dxi, deta)
}

impl TetMesh {
    fn corners(&self, t: &Tri6) -> [Vec3; 6] {
        t.nodes.map(|n| self.nodes[n as usize])
    }

    /// The triangle integrated over: at each point of the rule, its share of the area as a
    /// vector square to the surface (as the triangle's corners wind), and the weight each
    /// node has there.
    pub fn quadrature(&self, t: &Tri6) -> [(Vec3, [f64; 6]); 6] {
        let p = self.corners(t);
        RULE.map(|(xi, eta, w)| {
            let (n, dxi, deta) = shape(xi, eta);
            let along = |d: &[f64; 6]| (0..6).fold(Vec3::ZERO, |v, i| v + p[i] * d[i]);
            // (The reference triangle has area 1/2.)
            (along(&dxi).cross(along(&deta)) * (0.5 * w), n)
        })
    }

    /// A piece of a curve integrated along: at each of three points, its share of the length
    /// and the weight each of its nodes has there.
    pub fn along(&self, l: &Line3) -> [(f64, [f64; 3]); 3] {
        let p = l.nodes.map(|n| self.nodes[n as usize]);
        // (Gauss's three points on -1..1; the nodes are at -1, 1 and 0.)
        let at = 0.6f64.sqrt();
        [(-at, 5.0 / 9.0), (0.0, 8.0 / 9.0), (at, 5.0 / 9.0)].map(|(x, w)| {
            let n = [x * (x - 1.0) / 2.0, x * (x + 1.0) / 2.0, 1.0 - x * x];
            let d = [x - 0.5, x + 0.5, -2.0 * x];
            let tangent = p[0] * d[0] + p[1] * d[1] + p[2] * d[2];
            (tangent.len() * w, n)
        })
    }

    /// The length of a piece of a curve, bent as its middle node makes it.
    pub fn length(&self, l: &Line3) -> f64 {
        self.along(l).iter().map(|(dl, _)| dl).sum()
    }

    /// The area of a boundary triangle, curved as its mid-side nodes make it.
    pub fn area(&self, t: &Tri6) -> f64 {
        self.quadrature(t).iter().map(|(da, _)| da.len()).sum()
    }

    /// The middle of a boundary triangle (on its surface, not on the flat between its corners).
    pub fn middle(&self, t: &Tri6) -> Vec3 {
        let (n, _, _) = shape(1.0 / 3.0, 1.0 / 3.0);
        let p = self.corners(t);
        (0..6).fold(Vec3::ZERO, |v, i| v + p[i] * n[i])
    }

    /// For each boundary triangle, +1 when its corners wind so that its normal points out of
    /// the solid, -1 when into it: found from the tetrahedron it is a face of.
    pub fn outward(&self) -> Result<Vec<f64>, String> {
        let key = |a: u32, b: u32, c: u32| {
            let mut k = [a, b, c];
            k.sort_unstable();
            k
        };
        // Each face of each tetrahedron, with the corner that is not on it.
        let mut opposite: HashMap<[u32; 3], u32> = HashMap::with_capacity(self.tets.len() * 4);
        for t in &self.tets {
            for (face, other) in [([0, 1, 2], 3), ([0, 1, 3], 2), ([0, 2, 3], 1), ([1, 2, 3], 0)] {
                opposite.insert(key(t[face[0]], t[face[1]], t[face[2]]), t[other]);
            }
        }
        self.tris
            .iter()
            .map(|t| {
                let [a, b, c] = [t.nodes[0], t.nodes[1], t.nodes[2]];
                let inside = *opposite.get(&key(a, b, c)).ok_or("a triangle of the mesh's boundary is on no element")?;
                let (pa, pb, pc, pd) = (self.nodes[a as usize], self.nodes[b as usize], self.nodes[c as usize], self.nodes[inside as usize]);
                Ok(if (pb - pa).cross(pc - pa).dot(pd - pa) > 0.0 { -1.0 } else { 1.0 })
            })
            .collect()
    }

    /// The size of the box the mesh fits in, corner to corner.
    pub fn extent(&self) -> f64 {
        let Some(first) = self.nodes.first() else { return 0.0 };
        let (lo, hi) = self.nodes.iter().fold((*first, *first), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
        (hi - lo).len()
    }
}

/// Reads a mesh from the text of an MSH file (version 2.2). Elements other than ten-node
/// tetrahedra and six-node triangles are passed over (the mesher writes its curves and
/// corners too); a mesh of first-order elements is refused.
pub fn read_msh(text: &str) -> Result<TetMesh, String> {
    let format = text.lines().map(str::trim).skip_while(|l| *l != "$MeshFormat").nth(1).ok_or("the mesh file has no $MeshFormat section")?;
    let mut head = format.split_whitespace();
    let (version, kind) = (head.next().unwrap_or(""), head.next().unwrap_or(""));
    if !version.starts_with("2.") || kind != "0" {
        return Err(format!("the mesh file is not MSH version 2 as text (it says `{format}`)"));
    }

    let mut lines = text.lines().map(str::trim).skip_while(|l| *l != "$Nodes").skip(1);
    let count: usize = lines.next().and_then(|l| l.parse().ok()).ok_or("the mesh file's $Nodes section has no count")?;
    if count > MAX_NODES {
        return Err(format!("the mesh has {count} nodes: more than is read ({MAX_NODES})"));
    }
    // (As the file numbers them: any numbers, in any order.)
    let mut index: HashMap<u64, u32> = HashMap::with_capacity(count);
    let mut points: Vec<Vec3> = Vec::with_capacity(count);
    for _ in 0..count {
        let line = lines.next().ok_or("the mesh file ends inside its nodes")?;
        let mut f = line.split_whitespace();
        let id: u64 = f.next().and_then(|v| v.parse().ok()).ok_or_else(|| format!("not a node: `{line}`"))?;
        let mut xyz = [0.0f64; 3];
        for v in &mut xyz {
            *v = f.next().and_then(|v| v.parse().ok()).filter(|v: &f64| v.is_finite()).ok_or_else(|| format!("not a node: `{line}`"))?;
        }
        index.insert(id, u32::try_from(points.len()).map_err(|_| "too many nodes")?);
        points.push(Vec3::new(xyz[0], xyz[1], xyz[2]));
    }

    let mut lines = text.lines().map(str::trim).skip_while(|l| *l != "$Elements").skip(1);
    let count: usize = lines.next().and_then(|l| l.parse().ok()).ok_or("the mesh file has no $Elements section")?;
    let (mut tets, mut tris): (Vec<[u32; 10]>, Vec<Tri6>) = (Vec::new(), Vec::new());
    let mut pieces: Vec<Line3> = Vec::new();
    let mut first_order = false;
    for _ in 0..count.min(MAX_NODES * 8) {
        let line = lines.next().ok_or("the mesh file ends inside its elements")?;
        let f: Vec<u64> = line.split_whitespace().map(|v| v.parse().map_err(|_| format!("not an element: `{line}`"))).collect::<Result<_, _>>()?;
        let (Some(kind), Some(tags)) = (f.get(1), f.get(2)) else { return Err(format!("not an element: `{line}`")) };
        let tags = usize::try_from(*tags).map_err(|_| "not an element")?;
        let nodes = f.get(3 + tags..).ok_or_else(|| format!("not an element: `{line}`"))?;
        let node = |i: usize| -> Result<u32, String> {
            nodes.get(i).and_then(|id| index.get(id)).copied().ok_or_else(|| format!("an element refers to a node the file does not have: `{line}`"))
        };
        match kind {
            // (Its second tag is the surface it lies on.)
            9 if nodes.len() == 6 => {
                let surface = f.get(4).filter(|_| tags >= 2).copied().unwrap_or(0);
                let mut n = [0u32; 6];
                for (i, slot) in n.iter_mut().enumerate() {
                    *slot = node(i)?;
                }
                tris.push(Tri6 { surface: u32::try_from(surface).map_err(|_| "a surface number is too large")?, nodes: n });
            }
            // (A three-node line: its second tag is the curve it is a piece of.)
            8 if nodes.len() == 3 => {
                let curve = f.get(4).filter(|_| tags >= 2).copied().unwrap_or(0);
                pieces.push(Line3 { curve: u32::try_from(curve).map_err(|_| "a curve number is too large")?, nodes: [node(0)?, node(1)?, node(2)?] });
            }
            11 if nodes.len() == 10 => {
                let mut n = [0u32; 10];
                for (i, slot) in n.iter_mut().enumerate() {
                    *slot = node(i)?;
                }
                tets.push(n);
            }
            2 | 4 => first_order = true,
            _ => {}
        }
    }
    if tets.is_empty() {
        return Err(if first_order {
            "the mesh is of first-order elements: second-order ones (ten-node tetrahedra) are what is solved".into()
        } else {
            "the mesh has no elements: the mesher made nothing of the part".into()
        });
    }

    // Only the nodes the elements use, numbered from 0 in the order they are first met.
    let mut renumber: Vec<u32> = vec![u32::MAX; points.len()];
    let mut nodes: Vec<Vec3> = Vec::new();
    let mut used = |n: &mut u32| {
        let slot = &mut renumber[*n as usize];
        if *slot == u32::MAX {
            *slot = nodes.len() as u32;
            nodes.push(points[*n as usize]);
        }
        *n = *slot;
    };
    for t in &mut tets {
        t.iter_mut().for_each(&mut used);
    }
    for t in &mut tris {
        t.nodes.iter_mut().for_each(&mut used);
    }
    // (A piece of a curve is of the solid's own nodes: one that is not is left out.)
    pieces.retain(|l| l.nodes.iter().all(|n| renumber[*n as usize] != u32::MAX));
    for l in &mut pieces {
        l.nodes.iter_mut().for_each(|n| *n = renumber[*n as usize]);
    }
    Ok(TetMesh { nodes, tets, tris, lines: pieces })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// One tetrahedron with corners at the origin and one unit along each axis, with its four
    /// faces as boundary triangles on surfaces 1 to 4 (written as the mesher would).
    pub(crate) const ONE_TET: &str = "$MeshFormat\n2.2 0 8\n$EndMeshFormat\n$Nodes\n11\n\
        1 0 0 0\n2 1 0 0\n3 0 1 0\n4 0 0 1\n5 0.5 0 0\n6 0.5 0.5 0\n7 0 0.5 0\n8 0 0 0.5\n9 0 0.5 0.5\n10 0.5 0 0.5\n99 7 7 7\n\
        $EndNodes\n$Elements\n9\n\
        1 15 2 0 1 1\n\
        7 8 2 0 1 1 2 5\n\
        8 8 2 0 2 2 3 6\n\
        9 8 2 0 3 3 1 7\n\
        2 9 2 0 1 1 3 2 7 6 5\n\
        3 9 2 0 2 1 2 4 5 10 8\n\
        4 9 2 0 3 1 4 3 8 9 7\n\
        5 9 2 0 4 2 3 4 6 9 10\n\
        6 11 2 0 1 1 2 3 4 5 6 7 8 9 10\n\
        $EndElements\n";

    #[test]
    fn a_mesh_is_read_from_the_meshers_file() {
        let m = read_msh(ONE_TET).unwrap();
        assert_eq!((m.nodes.len(), m.tets.len(), m.tris.len()), (10, 1, 4), "the node no element uses is left out");
        // The pieces of its curves: the three sides of the bottom, each one piece.
        assert_eq!(m.lines.iter().map(|l| (l.curve, l.nodes)).collect::<Vec<_>>(), [(1, [0, 1, 4]), (2, [1, 2, 5]), (3, [2, 0, 6])]);
        let lengths: Vec<f64> = m.lines.iter().map(|l| m.length(l)).collect();
        assert!((lengths[0] - 1.0).abs() < 1e-12 && (lengths[1] - 2f64.sqrt()).abs() < 1e-12, "{lengths:?}");
        // A straight piece under an even load: a sixth on each end and two thirds on the middle.
        let mut share = [0.0; 3];
        for (dl, n) in m.along(&m.lines[0]) {
            (0..3).for_each(|i| share[i] += dl * n[i]);
        }
        assert!((share[0] - 1.0 / 6.0).abs() < 1e-12 && (share[1] - 1.0 / 6.0).abs() < 1e-12 && (share[2] - 2.0 / 3.0).abs() < 1e-12, "{share:?}");
        assert_eq!(m.tets[0], [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
        assert_eq!(m.tris.iter().map(|t| t.surface).collect::<Vec<_>>(), [1, 2, 3, 4]);
        assert_eq!(m.nodes[1], Vec3::X);
        // Areas: three faces of a half, and the slanted one.
        let areas: Vec<f64> = m.tris.iter().map(|t| m.area(t)).collect();
        for (a, want) in areas.iter().zip([0.5, 0.5, 0.5, 3f64.sqrt() / 2.0]) {
            assert!((a - want).abs() < 1e-12, "{areas:?}");
        }
        assert!((m.middle(&m.tris[0]) - Vec3::new(1.0 / 3.0, 1.0 / 3.0, 0.0)).len() < 1e-12);
        // Every one of them was written winding outward.
        assert_eq!(m.outward().unwrap(), [1.0, 1.0, 1.0, 1.0]);
        assert!((m.extent() - 3f64.sqrt()).abs() < 1e-12);
    }

    #[test]
    fn a_triangle_wound_inward_is_told_from_one_wound_outward() {
        // The bottom face with two corners exchanged (and the mid-side nodes with them).
        let flipped = ONE_TET.replace("2 9 2 0 1 1 3 2 7 6 5", "2 9 2 0 1 1 2 3 5 6 7");
        let m = read_msh(&flipped).unwrap();
        assert_eq!(m.outward().unwrap(), [-1.0, 1.0, 1.0, 1.0]);
        assert!((m.area(&m.tris[0]) - 0.5).abs() < 1e-12);
    }

    #[test]
    fn a_curved_triangle_has_the_area_of_its_curve() {
        // A quarter of a cylinder of radius 1 and height 1 as two triangles would be coarse:
        // here one triangle whose mid-side nodes are pushed out onto a sphere of radius 1.
        let on = |x: f64, y: f64, z: f64| {
            let v = Vec3::new(x, y, z);
            v * (1.0 / v.len())
        };
        let nodes = vec![Vec3::X, Vec3::Y, Vec3::Z, on(1.0, 1.0, 0.0), on(0.0, 1.0, 1.0), on(1.0, 0.0, 1.0)];
        let m = TetMesh { nodes, tets: Vec::new(), tris: vec![Tri6 { surface: 1, nodes: [0, 1, 2, 3, 4, 5] }], lines: Vec::new() };
        let flat = 3f64.sqrt() / 2.0;
        let sphere = std::f64::consts::PI / 2.0;
        let area = m.area(&m.tris[0]);
        assert!(area > flat * 1.3 && (area - sphere).abs() < 0.2, "far from the flat's {flat}, near the sphere's {sphere}: {area}");
        // Its middle is near the sphere, not on the flat between its corners.
        assert!(m.middle(&m.tris[0]).len() > 0.85, "{}", m.middle(&m.tris[0]).len());
    }

    #[test]
    fn what_is_not_a_mesh_is_refused_in_words() {
        let told = |text: &str, what: &str| {
            let e = read_msh(text).unwrap_err();
            assert!(e.contains(what), "{e}");
        };
        told("hello", "no $MeshFormat section");
        told(&ONE_TET.replace("2.2 0 8", "4.1 0 8"), "not MSH version 2 as text");
        told(&ONE_TET.replace("2.2 0 8", "2.2 1 8"), "not MSH version 2 as text");
        told(&ONE_TET.replace("4 0 0 1\n", "4 0 0 nan\n"), "not a node");
        told(&ONE_TET.replace("6 11 2 0 1 1 2 3 4 5 6 7 8 9 10", "6 11 2 0 1 1 2 3 4 5 6 7 8 9 77"), "a node the file does not have");
        told(&ONE_TET.replace("6 11 2 0 1 1 2 3 4 5 6 7 8 9 10", "6 4 2 0 1 1 2 3 4"), "first-order");
        told(&ONE_TET.replace("6 11 2 0 1 1 2 3 4 5 6 7 8 9 10\n", "6 15 2 0 1 1\n"), "no elements");
        told(&ONE_TET.replace("$Nodes\n11\n", "$Nodes\n99999999999\n"), "more than is read");
        let cut = &ONE_TET[..ONE_TET.find("9 0 0.5 0.5").unwrap()];
        told(cut, "ends inside its nodes");
    }
}
