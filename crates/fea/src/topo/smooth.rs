//! Smoothing the kept surface (DEC-123): Taubin's two steps (lambda 0.5, mu -0.53) on the
//! vertices that are not fixed, with umbrella weights, `pairs` times; it keeps the volume
//! about where Laplace's alone shrinks it.
//!
//! CONTRACT: `smooth(skin, pairs)` moves no fixed vertex, keeps the triangles (and so the
//! surface closed), and is deterministic.

use tenon_geom::Vec3;

use super::Skin;

/// The share of the way each vertex is moved toward the middle of its neighbours in the first
/// step of a pair, and in the second (less than nothing: away from it, a little further).
const LAMBDA: f64 = 0.5;
const MU: f64 = -0.53;

/// Smooths the surface `pairs` times: each time every vertex that is not fixed is moved half
/// way toward the middle of the vertices it shares a side with, then away from their new
/// middle by a little more than half as far. That takes out the roughness without the
/// shrinking the first step alone would give.
pub fn smooth(skin: &mut Skin, pairs: usize) {
    if pairs == 0 {
        return;
    }
    let near = Near::of(skin);
    for _ in 0..pairs {
        pass(&mut skin.positions, &near, &skin.fixed, LAMBDA);
        pass(&mut skin.positions, &near, &skin.fixed, MU);
    }
}

/// The vertices each vertex shares a side with, in order: those of vertex `i` are
/// `list[start[i]..start[i + 1]]`.
struct Near {
    start: Vec<usize>,
    list: Vec<u32>,
}

impl Near {
    fn of(skin: &Skin) -> Near {
        let n = skin.positions.len();
        let mut pairs: Vec<(u32, u32)> = skin
            .triangles
            .iter()
            .flat_map(|[a, b, c]| [(*a, *b), (*b, *c), (*c, *a)])
            .filter(|(a, b)| a != b && (*a as usize) < n && (*b as usize) < n)
            .flat_map(|(a, b)| [(a, b), (b, a)])
            .collect();
        pairs.sort_unstable();
        pairs.dedup();
        let mut start = vec![0usize; n + 1];
        for (a, _) in &pairs {
            start[*a as usize + 1] += 1;
        }
        for i in 0..n {
            start[i + 1] += start[i];
        }
        Near { start, list: pairs.into_iter().map(|(_, b)| b).collect() }
    }
}

/// One step: every vertex that is not fixed moved `factor` of the way toward the middle of its
/// neighbours, all from where they were before the step.
fn pass(positions: &mut [Vec3], near: &Near, fixed: &[bool], factor: f64) {
    let moved: Vec<Vec3> = positions
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let around = near.start.get(i..i + 2).and_then(|s| near.list.get(s[0]..s[1])).unwrap_or(&[]);
            if around.is_empty() || fixed.get(i) == Some(&true) {
                return *p;
            }
            let middle = around.iter().fold(Vec3::ZERO, |m, j| m + positions[*j as usize]) * (1.0 / around.len() as f64);
            *p + (middle - *p) * factor
        })
        .collect();
    positions.copy_from_slice(&moved);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topo::iso::extract;
    use crate::topo::iso::tests::{Lcg, random_field};

    /// A ball of radius `r`: an octahedron with each triangle split in four `levels` times,
    /// pushed out onto the sphere, each vertex then moved in or out by up to `rough` of `r`.
    fn ball(r: f64, levels: usize, rough: f64) -> Skin {
        let mut positions = vec![Vec3::X, Vec3::Y, Vec3::Z, -Vec3::X, -Vec3::Y, -Vec3::Z];
        let mut triangles: Vec<[u32; 3]> = vec![[0, 1, 2], [1, 3, 2], [3, 4, 2], [4, 0, 2], [1, 0, 5], [3, 1, 5], [4, 3, 5], [0, 4, 5]];
        for _ in 0..levels {
            let mut mids = std::collections::BTreeMap::new();
            let mut mid = |a: u32, b: u32, positions: &mut Vec<Vec3>| -> u32 {
                *mids.entry((a.min(b), a.max(b))).or_insert_with(|| {
                    positions.push((positions[a as usize] + positions[b as usize]).normalized());
                    (positions.len() - 1) as u32
                })
            };
            let mut finer = Vec::with_capacity(triangles.len() * 4);
            for [a, b, c] in triangles {
                let (ab, bc, ca) = (mid(a, b, &mut positions), mid(b, c, &mut positions), mid(c, a, &mut positions));
                finer.extend([[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]]);
            }
            triangles = finer;
        }
        let mut lcg = Lcg(31);
        let positions: Vec<Vec3> = positions.iter().map(|p| *p * (r * (1.0 + rough * (2.0 * lcg.next() - 1.0)))).collect();
        let fixed = vec![false; positions.len()];
        Skin { positions, triangles, fixed }
    }

    /// How far the vertices are from a sphere about the origin: the spread of their distances
    /// from it, as a share of their mean.
    fn roughness(skin: &Skin) -> f64 {
        let d: Vec<f64> = skin.positions.iter().map(|p| p.len()).collect();
        let mean = d.iter().sum::<f64>() / d.len() as f64;
        (d.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / d.len() as f64).sqrt() / mean
    }

    #[test]
    fn taubin_keeps_a_blobs_volume_where_laplace_shrinks_it() {
        let rough = ball(10.0, 4, 0.03);
        assert!(rough.is_closed() && rough.triangles.len() == 2048);
        let before = rough.volume();
        assert!((before / (4.0 / 3.0 * std::f64::consts::PI * 1000.0) - 1.0).abs() < 0.03, "{before}");
        let mut taubin = rough.clone();
        smooth(&mut taubin, 10);
        assert!((taubin.volume() / before - 1.0).abs() < 0.02, "{} against {before}", taubin.volume());
        assert!(roughness(&taubin) < 0.5 * roughness(&rough), "{} against {}", roughness(&taubin), roughness(&rough));
        // As many steps toward the middle alone.
        let mut laplace = rough.clone();
        let near = Near::of(&laplace);
        for _ in 0..20 {
            pass(&mut laplace.positions, &near, &laplace.fixed, LAMBDA);
        }
        assert!(laplace.volume() < 0.95 * before, "{} against {before}", laplace.volume());
        // No pairs: nothing moves.
        let mut none = rough.clone();
        smooth(&mut none, 0);
        assert_eq!(none, rough);
    }

    #[test]
    fn fixed_vertices_do_not_move() {
        let mut skin = ball(5.0, 3, 0.05);
        skin.fixed = (0..skin.positions.len()).map(|i| i % 4 == 0).collect();
        let before = skin.clone();
        smooth(&mut skin, 10);
        for (i, (p, q)) in skin.positions.iter().zip(&before.positions).enumerate() {
            if skin.fixed[i] {
                assert_eq!([p.x.to_bits(), p.y.to_bits(), p.z.to_bits()], [q.x.to_bits(), q.y.to_bits(), q.z.to_bits()], "vertex {i}");
            } else {
                assert!(p.dist(*q) > 1e-6, "vertex {i} did not move");
            }
        }
        assert_eq!((&skin.triangles, &skin.fixed), (&before.triangles, &before.fixed));
        // Every vertex fixed: none moves.
        let mut all = before.clone();
        all.fixed = vec![true; all.positions.len()];
        let still = all.clone();
        smooth(&mut all, 10);
        assert_eq!(all, still);
    }

    #[test]
    fn smoothing_keeps_the_skin_closed() {
        let made = extract(&random_field([5, 4, 3], 8), 0.5, 0).skin;
        assert!(made.is_closed() && made.fixed.iter().any(|f| *f) && made.fixed.iter().any(|f| !*f));
        let mut smoothed = made.clone();
        smooth(&mut smoothed, 10);
        assert_eq!(smoothed.triangles, made.triangles);
        assert!(smoothed.is_closed());
        assert!(smoothed.positions.iter().all(|p| p.is_finite()));
        assert!(smoothed.volume() > 0.0 && (smoothed.volume() / made.volume() - 1.0).abs() < 0.1, "{} against {}", smoothed.volume(), made.volume());
        // Smoothed again from the same: the same to the bit.
        let mut again = made.clone();
        smooth(&mut again, 10);
        assert_eq!(again, smoothed);
        // Triangles that name vertices it does not have are passed over.
        let mut odd = made.clone();
        odd.triangles.push([0, 1, 1_000_000]);
        smooth(&mut odd, 2);
        assert!(odd.positions.iter().all(|p| p.is_finite()));
    }
}
