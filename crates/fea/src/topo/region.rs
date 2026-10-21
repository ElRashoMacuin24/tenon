//! What must stay solid (DEC-123), and whether a part is the same on both sides of a plane.
//!
//! CONTRACT (filled in by the regions' implementer):
//! - `design(mesh, passive)`: each tetrahedron's volume (`TetMesh::volume`) and middle
//!   (`TetMesh::centroid`), with `passive` as given.
//! - `passive(problem, kept)`: true for every element with any of its ten nodes held or
//!   loaded (`Problem::held`, `Problem::forces`); for every element with a node on a boundary
//!   triangle whose mesh surface is in a kept set; and, for a kept set with a depth d, every
//!   element whose middle is within d of one of that set's boundary triangles (their corner
//!   triangles; a grid of cell max(d, element size) to find them).
//! - `asymmetry(mesh, mirror)`: how far the boundary is from its mirror image: the largest,
//!   over boundary triangle corners, of the distance from the corner's image to the nearest
//!   boundary triangle (0 for a symmetric part).

use std::collections::{BTreeSet, HashMap};

use tenon_geom::Vec3;

use crate::faces::dist2_to_triangle;
use crate::mesh::TetMesh;
use crate::problem::Problem;

use super::{Design, Mirror};

/// What the optimizer works on, for a mesh and the elements that must stay solid.
pub fn design(mesh: &TetMesh, passive: Vec<bool>) -> Design {
    Design { volumes: mesh.tets.iter().map(|t| mesh.volume(t)).collect(), centroids: mesh.tets.iter().map(|t| mesh.centroid(t)).collect(), passive }
}

/// Which elements must stay solid: those under a support or a load, and those of the kept
/// faces (each a set of the mesh's surfaces, with how deep under them to keep, mm).
pub fn passive(problem: &Problem, kept: &[(Vec<u32>, Option<f64>)]) -> Vec<bool> {
    let mesh = &problem.mesh;
    let mut keep = vec![false; mesh.nodes.len()];
    let mut mark = |n: u32| {
        if let Some(k) = keep.get_mut(n as usize) {
            *k = true;
        }
    };
    problem.held.keys().chain(problem.forces.keys()).for_each(|n| mark(*n));
    for (surfaces, _) in kept {
        let surfaces: BTreeSet<u32> = surfaces.iter().copied().collect();
        mesh.tris.iter().filter(|t| surfaces.contains(&t.surface)).for_each(|t| t.nodes.iter().for_each(|n| mark(*n)));
    }
    let mut passive: Vec<bool> = mesh.tets.iter().map(|t| t.iter().any(|n| keep.get(*n as usize) == Some(&true))).collect();
    for (surfaces, depth) in kept {
        let Some(depth) = depth.filter(|d| d.is_finite() && *d > 0.0) else { continue };
        let surfaces: BTreeSet<u32> = surfaces.iter().copied().collect();
        let tris: Vec<[Vec3; 3]> =
            mesh.tris.iter().filter(|t| surfaces.contains(&t.surface)).map(|t| [0, 1, 2].map(|i| mesh.nodes[t.nodes[i] as usize])).collect();
        let Some(grid) = Grid::new(&tris, depth) else { continue };
        for (e, t) in mesh.tets.iter().enumerate() {
            if !passive[e] && grid.within(&tris, mesh.centroid(t), depth) {
                passive[e] = true;
            }
        }
    }
    passive
}

/// How far the part's boundary is from its image in `mirror` (mm): the farthest any corner of a
/// boundary triangle, seen in the plane, is from the boundary. 0 when the part is the same on
/// both sides.
pub fn asymmetry(mesh: &TetMesh, mirror: &Mirror) -> f64 {
    let tris: Vec<[Vec3; 3]> = mesh.tris.iter().map(|t| [0, 1, 2].map(|i| mesh.nodes[t.nodes[i] as usize])).collect();
    let Some(tree) = Tree::new(&tris) else { return 0.0 };
    let corners: BTreeSet<u32> = mesh.tris.iter().flat_map(|t| [t.nodes[0], t.nodes[1], t.nodes[2]]).collect();
    // (The farthest so far: a corner nearer than it to the boundary cannot change it, so its
    // search stops as soon as a triangle that near is found.)
    let mut worst = 0.0f64;
    for n in corners {
        let image = mirror.image(mesh.nodes[n as usize]);
        worst = worst.max(tree.nearest(&tris, image, worst * worst).sqrt());
    }
    worst
}

/// Triangles filed by the cells of a grid they cross, to find those near a point.
struct Grid {
    cell: f64,
    cells: HashMap<[i64; 3], Vec<u32>>,
}

impl Grid {
    /// A grid for finding triangles within `reach`: its cells are at least that wide and at
    /// least as wide as the longest side, so that each triangle crosses few cells and all that
    /// are within reach of a point are in the 27 cells about it.
    fn new(tris: &[[Vec3; 3]], reach: f64) -> Option<Grid> {
        let longest = tris.iter().flat_map(|[a, b, c]| [a.dist(*b), b.dist(*c), c.dist(*a)]).fold(0.0f64, f64::max);
        let cell = reach.max(longest);
        if tris.is_empty() || !(cell.is_finite() && cell > 0.0) {
            return None;
        }
        let mut grid = Grid { cell, cells: HashMap::new() };
        for (i, [a, b, c]) in tris.iter().enumerate() {
            let (lo, hi) = (grid.key(a.min(*b).min(*c)), grid.key(a.max(*b).max(*c)));
            for x in lo[0]..=hi[0] {
                for y in lo[1]..=hi[1] {
                    for z in lo[2]..=hi[2] {
                        grid.cells.entry([x, y, z]).or_default().push(i as u32);
                    }
                }
            }
        }
        Some(grid)
    }

    fn key(&self, p: Vec3) -> [i64; 3] {
        [p.x, p.y, p.z].map(|v| (v / self.cell).floor() as i64)
    }

    /// Whether any of the triangles is within `reach` of `p` (`reach` no more than a cell).
    fn within(&self, tris: &[[Vec3; 3]], p: Vec3, reach: f64) -> bool {
        let [x, y, z] = self.key(p);
        let near = |i: &u32| tris.get(*i as usize).is_some_and(|[a, b, c]| dist2_to_triangle(p, *a, *b, *c) <= reach * reach);
        (x - 1..=x + 1).any(|i| (y - 1..=y + 1).any(|j| (z - 1..=z + 1).any(|k| self.cells.get(&[i, j, k]).is_some_and(|v| v.iter().any(near)))))
    }
}

/// Triangles in boxes within boxes, to find the nearest to a point wherever it is.
struct Tree {
    nodes: Vec<Branch>,
    order: Vec<u32>,
}

/// A box of the tree: the triangles `order[first..first + count]` when it is a leaf, else the
/// two boxes it is split into.
struct Branch {
    lo: Vec3,
    hi: Vec3,
    first: u32,
    count: u32,
    split: Option<(u32, u32)>,
}

/// The most triangles a leaf box holds.
const LEAF: usize = 4;

impl Tree {
    fn new(tris: &[[Vec3; 3]]) -> Option<Tree> {
        if tris.is_empty() {
            return None;
        }
        let mut tree = Tree { nodes: Vec::new(), order: (0..tris.len() as u32).collect() };
        let middles: Vec<Vec3> = tris.iter().map(|[a, b, c]| (*a + *b + *c) * (1.0 / 3.0)).collect();
        tree.build(tris, &middles, 0, tris.len());
        Some(tree)
    }

    /// Adds the box for `order[first..end]` and those under it; gives its place.
    fn build(&mut self, tris: &[[Vec3; 3]], middles: &[Vec3], first: usize, end: usize) -> u32 {
        let run = &mut self.order[first..end];
        let (lo, hi) = run
            .iter()
            .map(|i| &tris[*i as usize])
            .fold((Vec3::new(f64::MAX, f64::MAX, f64::MAX), Vec3::new(f64::MIN, f64::MIN, f64::MIN)), |(lo, hi), [a, b, c]| {
                (lo.min(*a).min(*b).min(*c), hi.max(*a).max(*b).max(*c))
            });
        let at = self.nodes.len() as u32;
        self.nodes.push(Branch { lo, hi, first: first as u32, count: (end - first) as u32, split: None });
        if end - first <= LEAF {
            return at;
        }
        // Split at the middle triangle along the way the box is longest (ties by number, so
        // the tree is the same each time).
        let size = hi - lo;
        let along = |p: Vec3| {
            if size.x >= size.y && size.x >= size.z {
                p.x
            } else if size.y >= size.z {
                p.y
            } else {
                p.z
            }
        };
        let half = (end - first) / 2;
        run.select_nth_unstable_by(half, |a, b| along(middles[*a as usize]).total_cmp(&along(middles[*b as usize])).then(a.cmp(b)));
        let left = self.build(tris, middles, first, first + half);
        let right = self.build(tris, middles, first + half, end);
        self.nodes[at as usize].split = Some((left, right));
        at
    }

    /// The square of the distance from `p` to the nearest triangle. The search stops as soon as
    /// it finds one whose square is `enough` or less, and gives that one's (for a caller that
    /// needs no nearer).
    fn nearest(&self, tris: &[[Vec3; 3]], p: Vec3, enough: f64) -> f64 {
        let gap = |b: &Branch| {
            let d = (b.lo - p).max(p - b.hi).max(Vec3::ZERO);
            d.len2()
        };
        let mut best = f64::INFINITY;
        let mut stack = vec![0u32];
        while let Some(i) = stack.pop() {
            let Some(b) = self.nodes.get(i as usize) else { continue };
            if gap(b) >= best {
                continue;
            }
            match b.split {
                Some((l, r)) => {
                    // (The nearer box is looked in first.)
                    let (dl, dr) = (self.nodes.get(l as usize).map_or(f64::INFINITY, gap), self.nodes.get(r as usize).map_or(f64::INFINITY, gap));
                    stack.extend(if dl <= dr { [r, l] } else { [l, r] });
                }
                None => {
                    for t in &self.order[b.first as usize..(b.first + b.count) as usize] {
                        let [a, bb, c] = tris[*t as usize];
                        best = best.min(dist2_to_triangle(p, a, bb, c));
                    }
                    if best <= enough {
                        return best;
                    }
                }
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::problem::Material;

    fn problem(mesh: TetMesh, held: &[u32], loaded: &[u32]) -> Problem {
        Problem {
            mesh,
            material: Material::of(200.0, 0.3, 7.85),
            held: held.iter().map(|n| (*n, vec![Vec3::X])).collect(),
            forces: loaded.iter().map(|n| (*n, Vec3::new(0.0, -10.0, 0.0))).collect::<BTreeMap<_, _>>(),
            gravity: None,
            spin: None,
        }
    }

    /// The node of a mesh at `p`.
    fn node_at(mesh: &TetMesh, p: Vec3) -> u32 {
        mesh.nodes.iter().position(|q| q.dist(p) < 1e-12).unwrap() as u32
    }

    #[test]
    fn a_design_has_each_elements_volume_and_middle() {
        let mesh = TetMesh::block([2, 1, 1], Vec3::new(4.0, 2.0, 2.0));
        let passive: Vec<bool> = (0..mesh.tets.len()).map(|e| e % 3 == 0).collect();
        let d = design(&mesh, passive.clone());
        assert_eq!((d.volumes.len(), d.centroids.len()), (12, 12));
        assert_eq!(d.passive, passive);
        assert!(d.volumes.iter().all(|v| (v - 8.0 / 6.0).abs() < 1e-12), "{:?}", d.volumes);
        assert!((d.volumes.iter().sum::<f64>() - 16.0).abs() < 1e-12);
        for (c, t) in d.centroids.iter().zip(&mesh.tets) {
            assert_eq!(*c, mesh.centroid(t));
        }
        // A curved element's volume is its own, not its corners'.
        let mut bent = mesh.clone();
        let mid = bent.tets[0][4] as usize;
        bent.nodes[mid] = bent.nodes[mid] + Vec3::new(0.1, -0.2, -0.3);
        let d = design(&bent, Vec::new());
        assert!(bent.tets.iter().zip(&d.volumes).all(|(t, v)| *v == bent.volume(t)));
        assert!(bent.tets.iter().zip(&d.volumes).any(|(t, v)| (v - bent.corner_volume(t)).abs() > 1e-3), "{:?}", d.volumes);
    }

    #[test]
    fn held_and_loaded_nodes_keep_their_elements() {
        let mesh = TetMesh::block([4, 4, 4], Vec3::new(4.0, 4.0, 4.0));
        // A corner of the box, a corner inside it, and the middle of a side of one element.
        let corner = node_at(&mesh, Vec3::ZERO);
        let inside = node_at(&mesh, Vec3::new(2.0, 2.0, 2.0));
        let mid = mesh.tets[200][7];
        let having = |n: u32| -> Vec<usize> { (0..mesh.tets.len()).filter(|e| mesh.tets[*e].contains(&n)).collect() };
        // (An inside corner is a corner of 24 elements as the cubes are divided; the box's
        // first corner of the 6 of its cube.)
        assert_eq!((having(corner).len(), having(inside).len()), (6, 24));
        let p = passive(&problem(mesh.clone(), &[corner], &[inside, mid]), &[]);
        let mut want = vec![false; mesh.tets.len()];
        for e in having(corner).into_iter().chain(having(inside)).chain(having(mid)) {
            want[e] = true;
        }
        assert_eq!(p, want);
        assert!(p[200]);
        // Nothing held or loaded: nothing kept.
        assert!(passive(&problem(mesh.clone(), &[], &[]), &[]).iter().all(|k| !k));
        // A node the mesh does not have is passed over.
        assert!(passive(&problem(mesh, &[999_999], &[]), &[]).iter().all(|k| !k));
    }

    #[test]
    fn kept_faces_keep_a_layer_and_a_depth() {
        let mesh = TetMesh::block([4, 4, 4], Vec3::new(4.0, 4.0, 4.0));
        let at = |e: usize| mesh.centroid(&mesh.tets[e]);
        // The face at x = 0 (surface 1): the elements with a node on it, the first layer of
        // cubes (each element of a cube has the cube's first corner).
        let p = passive(&problem(mesh.clone(), &[], &[]), &[(vec![1], None)]);
        assert_eq!(p.iter().filter(|k| **k).count(), 6 * 16);
        assert!((0..mesh.tets.len()).all(|e| p[e] == (at(e).x < 1.0)));
        // With a depth of 1.6: the elements whose middles are within it, part of the second
        // layer (their middles are at 1.25, 1.5 and 1.75).
        let p = passive(&problem(mesh.clone(), &[], &[]), &[(vec![1], Some(1.6))]);
        assert!((0..mesh.tets.len()).all(|e| p[e] == (at(e).x <= 1.6)), "{p:?}");
        assert_eq!(p.iter().filter(|k| **k).count(), 6 * 16 + 4 * 16);
        // Two sets, one deep, one not: x = 4 (surface 2) to 1.3 and y = 0 (surface 3) as a layer.
        let p = passive(&problem(mesh.clone(), &[], &[]), &[(vec![2], Some(1.3)), (vec![3], None)]);
        let layer3 = |e: usize| mesh.tets[e].iter().any(|n| mesh.nodes[*n as usize].y == 0.0);
        let near2 = |e: usize| mesh.tets[e].iter().any(|n| mesh.nodes[*n as usize].x == 4.0) || 4.0 - at(e).x <= 1.3;
        assert!((0..mesh.tets.len()).any(|e| at(e).x < 3.0 && near2(e) && !layer3(e)), "the depth reaches the next layer");
        assert!((0..mesh.tets.len()).all(|e| p[e] == (layer3(e) || near2(e))));
        // Deeper than an element: the second layer and part of the third.
        let p = passive(&problem(mesh.clone(), &[], &[]), &[(vec![5], Some(2.6))]);
        assert!((0..mesh.tets.len()).all(|e| p[e] == (at(e).z <= 2.6)));
        // A depth of nothing, or not a number, is only the layer; a surface the mesh does not
        // have keeps nothing.
        let layer = passive(&problem(mesh.clone(), &[], &[]), &[(vec![1], None)]);
        assert_eq!(passive(&problem(mesh.clone(), &[], &[]), &[(vec![1], Some(0.0))]), layer);
        assert_eq!(passive(&problem(mesh.clone(), &[], &[]), &[(vec![1], Some(f64::NAN))]), layer);
        assert!(passive(&problem(mesh, &[], &[]), &[(vec![77], Some(3.0))]).iter().all(|k| !k));
    }

    #[test]
    fn a_symmetric_block_is_its_own_image_and_a_block_cut_short_is_off_by_the_cut() {
        let mesh = TetMesh::block([4, 2, 2], Vec3::new(4.0, 2.0, 2.0));
        let x = Mirror { point: Vec3::new(2.0, 0.0, 0.0), normal: Vec3::X };
        let y = Mirror { point: Vec3::new(0.0, 1.0, 0.0), normal: Vec3::Y };
        assert!(asymmetry(&mesh, &x) < 1e-12 && asymmetry(&mesh, &y) < 1e-12, "{} {}", asymmetry(&mesh, &x), asymmetry(&mesh, &y));
        // 3.5 long about x = 2: its end at 0 is seen at 4, half a unit past the other end.
        let short = TetMesh::block([7, 2, 2], Vec3::new(3.5, 2.0, 2.0));
        assert!((asymmetry(&short, &x) - 0.5).abs() < 1e-12, "{}", asymmetry(&short, &x));
        assert!(asymmetry(&short, &y) < 1e-12);
        // A plane far off: the far end is seen far away.
        let far = Mirror { point: Vec3::new(-10.0, 0.0, 0.0), normal: Vec3::X };
        assert!((asymmetry(&mesh, &far) - 24.0).abs() < 1e-12, "{}", asymmetry(&mesh, &far));
        // No boundary: nothing to compare.
        assert_eq!(asymmetry(&TetMesh::default(), &x), 0.0);
    }

    #[test]
    fn the_nearest_triangle_is_the_one_a_search_of_all_finds() {
        // A box with its nodes moved about, against a slanted plane: the tree's answer is the
        // plain search's, to the bit.
        let mut mesh = TetMesh::block([5, 3, 4], Vec3::new(5.0, 3.0, 4.0));
        let mut seed = 12345u64;
        let mut next = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 11) as f64 / (1u64 << 53) as f64 - 0.5
        };
        for p in &mut mesh.nodes {
            *p = *p + Vec3::new(next(), next(), next()) * 0.3;
        }
        let n = Vec3::new(1.0, 0.3, -0.2);
        let m = Mirror { point: Vec3::new(2.4, 1.0, 2.0), normal: n * (1.0 / n.len()) };
        let tris: Vec<[Vec3; 3]> = mesh.tris.iter().map(|t| [0, 1, 2].map(|i| mesh.nodes[t.nodes[i] as usize])).collect();
        let mut want = 0.0f64;
        for t in &mesh.tris {
            for c in &t.nodes[..3] {
                let q = m.image(mesh.nodes[*c as usize]);
                let d = tris.iter().map(|[a, b, c]| dist2_to_triangle(q, *a, *b, *c)).fold(f64::INFINITY, f64::min);
                want = want.max(d.sqrt());
            }
        }
        assert!(want > 0.1, "{want}");
        assert_eq!(asymmetry(&mesh, &m), want);
        // Each point's nearest, without stopping early.
        let tree = Tree::new(&tris).unwrap();
        for i in 0..200 {
            let q = Vec3::new(next() * 20.0 + 2.5, next() * 20.0 + 1.5, next() * 20.0 + 2.0);
            let all = tris.iter().map(|[a, b, c]| dist2_to_triangle(q, *a, *b, *c)).fold(f64::INFINITY, f64::min);
            assert_eq!(tree.nearest(&tris, q, 0.0), all, "{i}");
        }
    }
}
