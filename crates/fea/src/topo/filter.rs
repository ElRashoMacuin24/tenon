//! The density filter (DEC-123): each element's density seen as the weighted mean of the
//! densities around it, so that no member is thinner than about twice the radius and the
//! design does not break into a checkerboard. With planes of symmetry, an element's
//! neighbours include the mirror images of the others, so a design that starts the same on
//! both sides stays so.
//!
//! CONTRACT (filled in by the filter's implementer):
//! - `Filter::new(centroids, volumes, radius, mirrors)`: a sparse matrix A with rows sorted by
//!   column, a_ei = w_ei V_i / S_e, w_ei = sum over the mirror group g of
//!   max(0, r - |c_e - g(c_i)|), S_e = sum_i w_ei V_i (so A keeps a constant field constant).
//!   Neighbours are found on a uniform grid of cell `radius`, one query per image; pairs that
//!   appear under several images add into one entry. A radius not above zero, lengths that
//!   differ, or a table that would take more than 512 MB, are refused in words.
//! - `apply(x)`: A x. `back(g)`: A^T g (the chain rule back through the filter).
//! - `mirror_group(mirrors)`: the reflections, the identity first: 1, 2, 4 or 8 members for 0
//!   to 3 planes, each the planes to reflect in, in turn; planes not at right angles
//!   (|n_i . n_j| >= 1e-6) or more than three are refused in words.
//! - Deterministic: the same inputs give bit-identical outputs.

use tenon_geom::Vec3;

use super::Mirror;

/// The most memory the filter's table may take.
const MOST_BYTES: usize = 512 * 1024 * 1024;

/// The filter as a sparse matrix, row by row.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filter {
    /// Where each row starts in `near` and `coef`, and where the last one ends.
    start: Vec<u32>,
    /// The elements each row takes in, in order.
    near: Vec<u32>,
    /// How much of each it takes: a row's add up to 1.
    coef: Vec<f64>,
}

impl Filter {
    /// The filter for elements with these middles and volumes, over `radius` (mm), the same on
    /// both sides of each of `mirrors`.
    ///
    /// Each plane through the part about doubles the table and the time of each pass, not
    /// only near the plane: every element's image lands inside the part, and the neighbours of
    /// the image are other elements, each a new entry in the row.
    pub fn new(centroids: &[Vec3], volumes: &[f64], radius: f64, mirrors: &[Mirror]) -> Result<Filter, String> {
        Filter::within(centroids, volumes, radius, mirrors, MOST_BYTES)
    }

    /// As `new`, with the table refused when it would take more than `most` bytes.
    fn within(centroids: &[Vec3], volumes: &[f64], radius: f64, mirrors: &[Mirror], most: usize) -> Result<Filter, String> {
        if !(radius.is_finite() && radius > 0.0) {
            return Err(format!("the radius to smooth the densities over must be more than 0 (it is {radius})"));
        }
        if centroids.len() != volumes.len() {
            return Err(format!("each element needs a middle and a volume: there are {} middles and {} volumes", centroids.len(), volumes.len()));
        }
        if u32::try_from(centroids.len()).is_err() {
            return Err(format!("{} elements are more than the filter can take", centroids.len()));
        }
        if centroids.iter().any(|c| !c.is_finite()) {
            return Err("an element's middle is not a point".into());
        }
        if volumes.iter().any(|v| !(v.is_finite() && *v > 0.0)) {
            return Err("every element's volume must be more than 0".into());
        }
        let group = mirror_group(mirrors)?;
        let grid = Grid::new(centroids, radius);
        let mut f = Filter { start: Vec::with_capacity(centroids.len() + 1), near: Vec::new(), coef: Vec::new() };
        f.start.push(0);
        // One row: each neighbour and its weight, under each image in turn.
        let mut row: Vec<(u32, f64)> = Vec::new();
        for c in centroids {
            row.clear();
            for g in &group {
                // |c_e - g(c_i)| is |g'(c_e) - c_i|, g' the same reflections the other way
                // round: reflections keep lengths.
                let q = g.iter().rev().fold(*c, |p, m| m.image(p));
                grid.near(q, |i| {
                    let d = q.dist(centroids[i as usize]);
                    if d < radius {
                        row.push((i, radius - d));
                    }
                });
            }
            // By column, the images of one neighbour added up in the group's order.
            row.sort_by_key(|(i, _)| *i);
            row.dedup_by(|later, kept| {
                let same = later.0 == kept.0;
                if same {
                    kept.1 += later.1;
                }
                same
            });
            // Refused before the row is kept. (Left to themselves the lists would double as they
            // fill, past the limit by as much again: they are only let grow up to it.)
            let entries = f.near.len() + row.len();
            let Ok(end) = u32::try_from(entries) else { return Err(too_big(radius, centroids.len(), mirrors.len(), most)) };
            if 4 * (f.start.len() + 1) + 12 * entries > most {
                return Err(too_big(radius, centroids.len(), mirrors.len(), most));
            }
            if entries > f.near.capacity() {
                let room = most.saturating_sub(4 * f.start.capacity()) / 12;
                let grown = (2 * f.near.capacity()).min(room).max(entries);
                f.near.reserve_exact(grown - f.near.len());
                f.coef.reserve_exact(grown - f.coef.len());
            }
            let sum: f64 = row.iter().map(|(i, w)| w * volumes[*i as usize]).sum();
            for (i, w) in &row {
                f.near.push(*i);
                f.coef.push(w * volumes[*i as usize] / sum);
            }
            f.start.push(end);
        }
        Ok(f)
    }

    /// How many elements the filter is for.
    fn rows(&self) -> usize {
        self.start.len().saturating_sub(1)
    }

    /// Row `e`: the elements it takes in and how much of each.
    fn row(&self, e: usize) -> (&[u32], &[f64]) {
        let (a, b) = (self.start.get(e).copied().unwrap_or(0) as usize, self.start.get(e + 1).copied().unwrap_or(0) as usize);
        (self.near.get(a..b).unwrap_or(&[]), self.coef.get(a..b).unwrap_or(&[]))
    }

    /// The densities `x` seen through the filter: A x.
    pub fn apply(&self, x: &[f64]) -> Vec<f64> {
        (0..self.rows())
            .map(|e| {
                let (near, coef) = self.row(e);
                near.iter().zip(coef).map(|(i, a)| a * x.get(*i as usize).copied().unwrap_or(0.0)).sum()
            })
            .collect()
    }

    /// How a quantity that goes with the filtered densities goes with the densities: A^T g,
    /// for `g` its slope against each filtered density.
    pub fn back(&self, g: &[f64]) -> Vec<f64> {
        let mut out = vec![0.0; self.rows()];
        for e in 0..self.rows() {
            let ge = g.get(e).copied().unwrap_or(0.0);
            let (near, coef) = self.row(e);
            for (i, a) in near.iter().zip(coef) {
                if let Some(o) = out.get_mut(*i as usize) {
                    *o += a * ge;
                }
            }
        }
        out
    }
}

/// The refusal of a table too large to keep, for so many `planes` of symmetry.
fn too_big(radius: f64, elements: usize, planes: usize, most: usize) -> String {
    let radius = (radius * 1000.0).round() / 1000.0;
    let most = most / (1024 * 1024);
    match planes {
        0 => format!(
            "smoothing the densities of {elements} elements over {radius} mm would take more than {most} MB: ask for a smaller smallest member, or suppress the part's small holes and rounds"
        ),
        _ => format!(
            "smoothing the densities of {elements} elements over {radius} mm with {planes} plane{} of symmetry would take more than {most} MB: ask for a smaller smallest member, suppress the part's small holes and rounds, or take fewer planes (each plane through the part doubles the memory)",
            if planes == 1 { "" } else { "s" }
        ),
    }
}

/// The elements in cubes `cell` across, to find those near a point without looking at all.
struct Grid {
    cell: f64,
    /// The corners of the box the middles fit in.
    lo: Vec3,
    hi: Vec3,
    /// The cubes with elements in, in order (as z, y, x).
    cubes: Vec<[i64; 3]>,
    /// Where each cube's elements start in `order`, and where the last one's end.
    first: Vec<usize>,
    /// The elements, cube by cube, each cube's in element order.
    order: Vec<u32>,
}

impl Grid {
    fn new(points: &[Vec3], cell: f64) -> Grid {
        let lo = points.iter().copied().reduce(Vec3::min).unwrap_or(Vec3::ZERO);
        let hi = points.iter().copied().reduce(Vec3::max).unwrap_or(Vec3::ZERO);
        let mut g = Grid { cell, lo, hi, cubes: Vec::new(), first: Vec::new(), order: (0..points.len() as u32).collect() };
        let keys: Vec<[i64; 3]> = points.iter().map(|p| g.cube(*p)).collect();
        // (A stable sort: within a cube the elements stay in their order.)
        g.order.sort_by_key(|i| keys[*i as usize]);
        for (at, i) in g.order.iter().enumerate() {
            let key = keys[*i as usize];
            if g.cubes.last() != Some(&key) {
                g.cubes.push(key);
                g.first.push(at);
            }
        }
        g.first.push(g.order.len());
        g
    }

    /// The cube a point is in.
    fn cube(&self, p: Vec3) -> [i64; 3] {
        let at = |v: f64, from: f64| ((v - from) / self.cell).floor() as i64;
        [at(p.z, self.lo.z), at(p.y, self.lo.y), at(p.x, self.lo.x)]
    }

    /// Calls `found` with every element in the cube `q` is in and the 26 around it, in a fixed
    /// order: all those nearer to `q` than one cube across, and some more.
    fn near(&self, q: Vec3, mut found: impl FnMut(u32)) {
        let r = self.cell;
        if q.x < self.lo.x - r || q.y < self.lo.y - r || q.z < self.lo.z - r || q.x > self.hi.x + r || q.y > self.hi.y + r || q.z > self.hi.z + r {
            return;
        }
        let [z, y, x] = self.cube(q);
        for dz in -1..=1 {
            for dy in -1..=1 {
                // The three cubes along x are next to each other in the list.
                let (zz, yy) = (z.saturating_add(dz), y.saturating_add(dy));
                let (from, to) = ([zz, yy, x.saturating_sub(1)], [zz, yy, x.saturating_add(1)]);
                let mut c = self.cubes.partition_point(|k| *k < from);
                while self.cubes.get(c).is_some_and(|k| *k <= to) {
                    let (a, b) = (self.first.get(c).copied().unwrap_or(0), self.first.get(c + 1).copied().unwrap_or(0));
                    self.order.get(a..b).unwrap_or(&[]).iter().for_each(|i| found(*i));
                    c += 1;
                }
            }
        }
    }
}

/// The reflections of a group of planes, each the planes to reflect in, in turn.
///
/// For planes `[a, b, c]` the members are, in order: none, `[a]`, `[b]`, `[a, b]`, `[c]`,
/// `[a, c]`, `[b, c]`, `[a, b, c]`. Planes at right angles are reflected in in either order to
/// the same point, so these are all there are. The normals are made of unit length.
pub fn mirror_group(mirrors: &[Mirror]) -> Result<Vec<Vec<Mirror>>, String> {
    if mirrors.len() > 3 {
        return Err(format!("a shape can be the same on both sides of at most three planes, not {}", mirrors.len()));
    }
    let mut planes = Vec::with_capacity(mirrors.len());
    for m in mirrors {
        if !(m.point.is_finite() && m.normal.is_finite() && m.normal.len() > 1e-12) {
            return Err("a plane of symmetry is not in any one place or direction".into());
        }
        planes.push(Mirror { point: m.point, normal: m.normal.normalized() });
    }
    tenon_model::study::planes_square(&planes.iter().map(|m| m.normal).collect::<Vec<_>>())?;
    Ok((0..1usize << planes.len()).map(|set| planes.iter().enumerate().filter(|(i, _)| set >> i & 1 == 1).map(|(_, m)| *m).collect()).collect())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::mesh::TetMesh;

    /// A fixed sequence of numbers from 0 to 1 that look random.
    pub(crate) struct Lcg(pub u64);

    impl Lcg {
        pub fn next(&mut self) -> f64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (self.0 >> 11) as f64 / (1u64 << 53) as f64
        }
        pub fn many(&mut self, n: usize, lo: f64, hi: f64) -> Vec<f64> {
            (0..n).map(|_| lo + (hi - lo) * self.next()).collect()
        }
    }

    /// The middles and volumes of a block's elements.
    pub(crate) fn block(cells: [usize; 3], size: Vec3) -> (Vec<Vec3>, Vec<f64>) {
        let m = TetMesh::block(cells, size);
        (m.tets.iter().map(|t| m.centroid(t)).collect(), m.tets.iter().map(|t| m.volume(t)).collect())
    }

    fn dot(a: &[f64], b: &[f64]) -> f64 {
        a.iter().zip(b).map(|(x, y)| x * y).sum()
    }

    fn plane(point: Vec3, normal: Vec3) -> Mirror {
        Mirror { point, normal }
    }

    /// The filter written out in full, worked out pair by pair from its definition.
    fn by_hand(c: &[Vec3], v: &[f64], r: f64, mirrors: &[Mirror]) -> Vec<Vec<f64>> {
        let group = mirror_group(mirrors).unwrap();
        let image = |g: &[Mirror], p: Vec3| g.iter().fold(p, |p, m| m.image(p));
        (0..c.len())
            .map(|e| {
                let w: Vec<f64> = (0..c.len()).map(|i| group.iter().map(|g| (r - c[e].dist(image(g, c[i]))).max(0.0)).sum::<f64>()).collect();
                let s: f64 = (0..c.len()).map(|i| w[i] * v[i]).sum();
                (0..c.len()).map(|i| w[i] * v[i] / s).collect()
            })
            .collect()
    }

    #[test]
    fn the_filter_keeps_a_constant_field_constant() {
        let (c, mut v) = block([4, 3, 3], Vec3::new(8.0, 6.0, 6.0));
        let mut rng = Lcg(7);
        // Volumes that differ, so that the weights do.
        v.iter_mut().for_each(|v| *v *= 0.5 + rng.next());
        for mirrors in [
            vec![],
            vec![plane(Vec3::new(0.0, 0.0, 0.0), Vec3::X)],
            vec![plane(Vec3::new(8.0, 0.0, 0.0), Vec3::X), plane(Vec3::new(0.0, 3.0, 0.0), Vec3::Y)],
        ] {
            let f = Filter::new(&c, &v, 3.1, &mirrors).unwrap();
            assert_eq!(f.rows(), c.len());
            for value in [1.0, 0.37, 250.0] {
                let y = f.apply(&vec![value; c.len()]);
                assert!(
                    y.iter().all(|y| (y - value).abs() < 1e-12 * value),
                    "{:?}",
                    y.iter().map(|y| y - value).fold(0.0, |a: f64, b| a.max(b.abs()))
                );
            }
            // Each row in column order, each column once, and many in each row.
            for e in 0..f.rows() {
                let (near, coef) = f.row(e);
                assert!(near.windows(2).all(|w| w[0] < w[1]), "row {e}: {near:?}");
                assert!(near.contains(&(e as u32)) && near.len() > 10 && coef.iter().all(|a| *a > 0.0));
            }
        }
    }

    #[test]
    fn the_filter_is_its_definition() {
        let (corner, mut v) = block([3, 2, 2], Vec3::new(3.0, 2.0, 2.0));
        let mut rng = Lcg(11);
        v.iter_mut().for_each(|v| *v *= 0.5 + rng.next());
        // The block from the origin, the planes on its faces; then a longer one with its middle
        // at the origin, the planes through it, so that every element's images land inside,
        // some of them farther than the radius from the element.
        let (long, mut lv) = block([5, 3, 2], Vec3::new(5.0, 3.0, 2.0));
        lv.iter_mut().for_each(|v| *v *= 0.5 + rng.next());
        let middle: Vec<Vec3> = long.iter().map(|p| *p - Vec3::new(2.5, 1.5, 1.0)).collect();
        for (c, v) in [(corner, v), (middle, lv)] {
            for mirrors in [
                vec![],
                vec![plane(Vec3::new(0.0, 0.0, 0.0), Vec3::X)],
                vec![plane(Vec3::ZERO, Vec3::X), plane(Vec3::ZERO, Vec3::Y), plane(Vec3::ZERO, Vec3::Z)],
            ] {
                let f = Filter::new(&c, &v, 1.3, &mirrors).unwrap();
                let full = by_hand(&c, &v, 1.3, &mirrors);
                for (e, want) in full.iter().enumerate() {
                    let (near, coef) = f.row(e);
                    let mut got = vec![0.0; c.len()];
                    near.iter().zip(coef).for_each(|(i, a)| got[*i as usize] = *a);
                    assert!(got.iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-14), "row {e}");
                    // Only the elements with a weight are in the row.
                    assert_eq!(near.len(), want.iter().filter(|a| **a > 0.0).count());
                }
            }
        }
    }

    #[test]
    fn a_radius_below_the_spacing_is_the_identity() {
        let (c, v) = block([3, 3, 2], Vec3::new(3.0, 3.0, 2.0));
        let spacing = (0..c.len()).flat_map(|i| (0..i).map(move |j| (i, j))).map(|(i, j)| c[i].dist(c[j])).fold(f64::INFINITY, f64::min);
        assert!(spacing > 0.1);
        let x = Lcg(3).many(c.len(), 0.0, 1.0);
        for radius in [0.99 * spacing, 1e-9] {
            let f = Filter::new(&c, &v, radius, &[]).unwrap();
            assert_eq!(f.near, (0..c.len() as u32).collect::<Vec<_>>());
            assert!(f.coef.iter().all(|a| *a == 1.0));
            assert_eq!(f.apply(&x), x);
            assert_eq!(f.back(&x), x);
        }
        // Just above it, the nearest pairs take each other in.
        let f = Filter::new(&c, &v, 1.01 * spacing, &[]).unwrap();
        assert!(f.near.len() > c.len());
    }

    #[test]
    fn the_back_pass_is_the_transpose() {
        let (c, mut v) = block([4, 3, 2], Vec3::new(4.0, 3.0, 2.0));
        let mut rng = Lcg(5);
        v.iter_mut().for_each(|v| *v *= 0.5 + rng.next());
        for mirrors in [vec![], vec![plane(Vec3::new(4.0, 0.0, 0.0), Vec3::X), plane(Vec3::new(0.0, 0.0, 2.0), Vec3::Z)]] {
            let f = Filter::new(&c, &v, 1.6, &mirrors).unwrap();
            for _ in 0..5 {
                let (x, y) = (rng.many(c.len(), -1.0, 1.0), rng.many(c.len(), -1.0, 1.0));
                let (ax_y, x_aty) = (dot(&f.apply(&x), &y), dot(&x, &f.back(&y)));
                assert!((ax_y - x_aty).abs() < 1e-12 * ax_y.abs().max(1.0), "{ax_y} {x_aty}");
            }
            // Column by column: the back pass of a unit vector is a column of the matrix.
            let full = by_hand(&c, &v, 1.6, &mirrors);
            for i in [0, 7, c.len() - 1] {
                let mut unit = vec![0.0; c.len()];
                unit[i] = 1.0;
                let col = f.back(&unit);
                assert!((0..c.len()).all(|e| (col[e] - full[i][e]).abs() < 1e-14), "column {i}");
            }
        }
    }

    #[test]
    fn the_filter_is_the_same_each_time() {
        let (c, v) = block([5, 4, 3], Vec3::new(5.0, 4.0, 3.0));
        let mirrors = [plane(Vec3::new(0.0, 2.0, 0.0), Vec3::Y)];
        let (a, b) = (Filter::new(&c, &v, 1.7, &mirrors).unwrap(), Filter::new(&c, &v, 1.7, &mirrors).unwrap());
        assert_eq!(a.start, b.start);
        assert_eq!(a.near, b.near);
        assert!(a.coef.iter().zip(&b.coef).all(|(x, y)| x.to_bits() == y.to_bits()));
        let x = Lcg(9).many(c.len(), 0.0, 1.0);
        let bits = |v: Vec<f64>| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
        assert_eq!(bits(a.apply(&x)), bits(b.apply(&x)));
        assert_eq!(bits(a.back(&x)), bits(b.back(&x)));
    }

    #[test]
    fn a_mirrored_block_gets_a_mirrored_field() {
        // A block, then its images in one, two and three planes, made here: the element k of
        // copy j is the element k of the block reflected in copy j's planes.
        let (half, hv) = block([3, 3, 2], Vec3::new(3.0, 3.0, 2.0));
        let planes = [plane(Vec3::ZERO, Vec3::X), plane(Vec3::new(0.0, 3.0, 0.0), Vec3::Y), plane(Vec3::new(0.0, 0.0, 2.0), Vec3::Z)];
        let mut rng = Lcg(21);
        for k in 1..=3 {
            let group = mirror_group(&planes[..k]).unwrap();
            let c: Vec<Vec3> = group.iter().flat_map(|g| half.iter().map(|p| g.iter().fold(*p, |p, m| m.image(p)))).collect();
            let v: Vec<f64> = group.iter().flat_map(|_| hv.iter().copied()).collect();
            let x = rng.many(c.len(), 0.0, 1.0);
            let n = half.len();
            let worst = |f: &Filter| {
                let y = f.apply(&x);
                (0..c.len()).map(|e| (y[e] - y[e % n]).abs()).fold(0.0, f64::max)
            };
            let f = Filter::new(&c, &v, 1.5, &planes[..k]).unwrap();
            assert!(worst(&f) < 1e-12, "{k} planes: {}", worst(&f));
            // Without the planes the two sides see different neighbours.
            assert!(worst(&Filter::new(&c, &v, 1.5, &[]).unwrap()) > 1e-3);
        }
    }

    #[test]
    fn each_plane_through_the_part_doubles_the_rows_away_from_it() {
        // A block and its images in one, two and three planes, made here so that the whole is
        // the same on both sides of each plane, element for element (the images are exact: the
        // middles are in quarters).
        let (half, mut hv) = block([4, 4, 4], Vec3::new(4.0, 4.0, 4.0));
        let mut rng = Lcg(31);
        hv.iter_mut().for_each(|v| *v *= 0.5 + rng.next());
        let planes = [plane(Vec3::ZERO, Vec3::X), plane(Vec3::new(0.0, 4.0, 0.0), Vec3::Y), plane(Vec3::new(0.0, 0.0, 4.0), Vec3::Z)];
        let r = 1.6;
        for k in 1..=3 {
            let group = mirror_group(&planes[..k]).unwrap();
            let c: Vec<Vec3> = group.iter().flat_map(|g| half.iter().map(|p| g.iter().fold(*p, |p, m| m.image(p)))).collect();
            let v: Vec<f64> = group.iter().flat_map(|_| hv.iter().copied()).collect();
            let (plain, mirrored) = (Filter::new(&c, &v, r, &[]).unwrap(), Filter::new(&c, &v, r, &planes[..k]).unwrap());
            let mut away = 0;
            for (e, ce) in c.iter().enumerate() {
                let ((near0, coef0), (near, coef)) = (plain.row(e), mirrored.row(e));
                // Each image's neighbours are as many as the element's own, and some may be
                // the same ones.
                assert!(near.len() <= near0.len() << k, "row {e}");
                if planes[..k].iter().all(|m| (*ce - m.point).dot(m.normal).abs() >= r) {
                    away += 1;
                    // Away from the planes no two images share a neighbour: the row takes in a
                    // whole neighbourhood for each.
                    assert_eq!(near.len(), near0.len() << k, "row {e}");
                    // Its own neighbours keep their weights, shared out over 2^k as many.
                    for (i, a) in near0.iter().zip(coef0) {
                        let at = near.binary_search(i).unwrap();
                        assert!((coef[at] * (1 << k) as f64 - a).abs() < 1e-12 * a, "row {e}, column {i}");
                    }
                }
            }
            // Many are away from the planes: about 0.6^k of them.
            assert!(away * 8 > c.len(), "{k} planes: {away} of {}", c.len());
            // A table too large says to leave out a plane.
            let bytes = 4 * mirrored.start.len() + 12 * mirrored.near.len();
            assert!(Filter::within(&c, &v, r, &planes[..k], bytes).is_ok());
            assert_eq!(
                Filter::within(&c, &v, r, &planes[..k], bytes - 1).unwrap_err(),
                format!(
                    "smoothing the densities of {} elements over 1.6 mm with {k} plane{} of symmetry would take more than {} MB: ask for a smaller smallest member, suppress the part's small holes and rounds, or take fewer planes (each plane through the part doubles the memory)",
                    c.len(),
                    if k == 1 { "" } else { "s" },
                    (bytes - 1) >> 20
                )
            );
        }
    }

    #[test]
    fn planes_not_at_right_angles_are_refused() {
        let x = plane(Vec3::ZERO, Vec3::X);
        let tilted = plane(Vec3::ZERO, Vec3::new(1.0, 1.0, 0.0).normalized());
        let e = mirror_group(&[x, tilted]).unwrap_err();
        assert_eq!(e, "two of the planes of symmetry meet at 45°: they must be at right angles to each other");
        let e = mirror_group(&[x, plane(Vec3::new(3.0, 0.0, 0.0), Vec3::X)]).unwrap_err();
        assert!(e.contains("parallel") && e.contains("right angles"), "{e}");
        let e = mirror_group(&[x, plane(Vec3::ZERO, -Vec3::X)]).unwrap_err();
        assert!(e.contains("parallel"), "{e}");
        // Nearly square is square enough; not quite is not.
        assert!(mirror_group(&[x, plane(Vec3::ZERO, Vec3::new(5e-7, 1.0, 0.0))]).is_ok());
        assert_eq!(
            mirror_group(&[x, plane(Vec3::ZERO, Vec3::new(2e-6, 1.0, 0.0))]).unwrap_err(),
            "two of the planes of symmetry are a little off a right angle: they must be at right angles to each other"
        );
        let e = mirror_group(&[x, plane(Vec3::ZERO, Vec3::Y), plane(Vec3::ZERO, Vec3::Z), x]).unwrap_err();
        assert_eq!(e, "a shape can be the same on both sides of at most three planes, not 4");
        assert!(mirror_group(&[plane(Vec3::ZERO, Vec3::ZERO)]).unwrap_err().contains("direction"));
        // The filter says the same.
        let (c, v) = block([1, 1, 1], Vec3::new(1.0, 1.0, 1.0));
        assert!(Filter::new(&c, &v, 0.5, &[x, tilted]).unwrap_err().contains("right angles"));
    }

    #[test]
    fn the_mirror_group_has_two_four_or_eight_members() {
        let planes = [
            plane(Vec3::new(1.0, 0.0, 0.0), Vec3::X),
            plane(Vec3::new(0.0, -2.0, 0.0), Vec3::new(0.0, 3.0, 0.0)),
            plane(Vec3::new(0.0, 0.0, 5.0), Vec3::Z),
        ];
        let p = Vec3::new(0.3, 0.7, 1.9);
        let image = |g: &[Mirror], p: Vec3| g.iter().fold(p, |p, m| m.image(p));
        for k in 0..=3 {
            let group = mirror_group(&planes[..k]).unwrap();
            assert_eq!(group.len(), 1 << k);
            assert!(group[0].is_empty(), "the identity first");
            assert!(group.iter().all(|g| g.len() <= k && g.iter().all(|m| (m.normal.len() - 1.0).abs() < 1e-15)));
            // Each member puts the point somewhere of its own; each undoes itself; and any two
            // in turn are a third.
            let images: Vec<Vec3> = group.iter().map(|g| image(g, p)).collect();
            for (i, a) in images.iter().enumerate() {
                assert!(images.iter().skip(i + 1).all(|b| a.dist(*b) > 0.5), "{images:?}");
                assert!(image(&group[i], *a).dist(p) < 1e-12);
                for g in &group {
                    let twice = image(g, *a);
                    assert!(images.iter().any(|b| b.dist(twice) < 1e-12));
                }
            }
        }
        assert_eq!(mirror_group(&planes[..2]).unwrap()[3], vec![planes[0], Mirror { point: planes[1].point, normal: Vec3::Y }]);
    }

    #[test]
    fn a_radius_of_nothing_or_lengths_that_differ_are_refused() {
        let (c, v) = block([1, 1, 1], Vec3::new(1.0, 1.0, 1.0));
        for r in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(Filter::new(&c, &v, r, &[]).unwrap_err().starts_with("the radius to smooth the densities over must be more than 0"));
        }
        assert_eq!(Filter::new(&c, &v[1..], 0.5, &[]).unwrap_err(), "each element needs a middle and a volume: there are 6 middles and 5 volumes");
        let mut zero = v.clone();
        zero[2] = 0.0;
        assert_eq!(Filter::new(&c, &zero, 0.5, &[]).unwrap_err(), "every element's volume must be more than 0");
        assert_eq!(Filter::new(&[], &[], 0.5, &[]).unwrap(), Filter { start: vec![0], near: vec![], coef: vec![] });
    }

    #[test]
    fn a_table_too_large_is_refused_in_words() {
        let (c, v) = block([4, 4, 4], Vec3::new(4.0, 4.0, 4.0));
        let f = Filter::new(&c, &v, 2.0, &[]).unwrap();
        let bytes = 4 * f.start.len() + 12 * f.near.len();
        assert!(Filter::within(&c, &v, 2.0, &[], bytes).is_ok());
        let e = Filter::within(&c, &v, 2.0, &[], bytes - 1).unwrap_err();
        assert_eq!(
            e,
            format!(
                "smoothing the densities of 384 elements over 2 mm would take more than {} MB: ask for a smaller smallest member, or suppress the part's small holes and rounds",
                (bytes - 1) >> 20
            )
        );
        // The lists are not let grow past the limit to find that out, nor when the table fits.
        let fits = Filter::within(&c, &v, 2.0, &[], bytes).unwrap();
        assert!(4 * fits.start.capacity() + 8 * fits.coef.capacity() + 4 * fits.near.capacity() <= bytes, "{bytes}");
        assert!(too_big(1.5, 100_000, 0, MOST_BYTES).contains("more than 512 MB"));
        // With planes, leaving one out halves the table.
        assert_eq!(
            too_big(1.5004, 101_400, 1, MOST_BYTES),
            "smoothing the densities of 101400 elements over 1.5 mm with 1 plane of symmetry would take more than 512 MB: ask for a smaller smallest member, suppress the part's small holes and rounds, or take fewer planes (each plane through the part doubles the memory)"
        );
        assert!(too_big(2.0, 101_400, 3, MOST_BYTES).contains("over 2 mm with 3 planes of symmetry would take more than 512 MB: ask for a smaller"));
    }

    #[test]
    fn a_hundred_thousand_elements_are_filtered_in_moments() {
        // 26 x 26 x 25 cubes of six: 101,400 elements, each taking in its neighbours within one
        // and a half cubes; then the same with a plane of symmetry through the middle, as on a
        // part that is the same on both sides of it, where each element takes in its image's
        // neighbours too.
        let (c, v) = block([26, 26, 25], Vec3::new(26.0, 26.0, 25.0));
        let c: Vec<Vec3> = c.iter().map(|p| *p - Vec3::new(13.0, 13.0, 12.5)).collect();
        let longest = |f: &Filter| (0..f.rows()).map(|e| f.row(e).0.len()).max().unwrap();
        let began = std::time::Instant::now();
        let plain = Filter::new(&c, &v, 1.5, &[]).unwrap();
        let (entries, most) = (plain.near.len(), longest(&plain));
        drop(plain);
        let f = Filter::new(&c, &v, 1.5, &[plane(Vec3::ZERO, Vec3::X)]).unwrap();
        let x = f.apply(&vec![1.0; c.len()]);
        let built = began.elapsed();
        assert!(x.iter().all(|x| (x - 1.0).abs() < 1e-12));
        assert!(most < 100, "{most}");
        assert!(longest(&f) <= 2 * most, "{} against {most}", longest(&f));
        // The elements farther than the radius from the plane, nine in ten of them, take in two
        // neighbourhoods; those nearer, where the two overlap, fewer.
        let grown = f.near.len() as f64 / entries as f64;
        assert!(grown > 1.8 && grown < 2.0, "{grown}");
        assert!(4 * f.start.len() + 12 * f.near.len() < MOST_BYTES / 2);
        assert!(built.as_secs_f64() < 30.0, "{built:?}");
    }
}
