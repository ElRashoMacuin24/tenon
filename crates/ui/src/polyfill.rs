//! Filling an outline that has holes in it: the triangles a painter needs, since a painter fills
//! convex shapes only. Used to show a flat pattern as a sheet.
//!
//! The outlines are cut into level strips at every corner. Within a strip no edge starts or
//! ends, so the edges that cross it, taken from left to right, are by turns where the sheet
//! begins and where it ends: each pair bounds a trapezium, which is two triangles. Nothing has
//! to be joined up or searched for, so holes that touch the outline, or one another, at a point
//! are no special case.

use tenon_geom::Vec2;

/// Triangles that fill `outer` less its `holes`, each counter-clockwise. The outlines may be
/// given either way round.
pub(crate) fn triangulate(outer: &[Vec2], holes: &[Vec<Vec2>]) -> Vec<[Vec2; 3]> {
    // Every edge that is not level, from its lower end to its upper; and every level there is.
    let mut edges: Vec<(Vec2, Vec2)> = Vec::new();
    let mut levels: Vec<f64> = Vec::new();
    for poly in std::iter::once(outer).chain(holes.iter().map(Vec::as_slice)).filter(|p| p.len() >= 3) {
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            levels.push(a.y);
            if a.y < b.y {
                edges.push((a, b));
            } else if b.y < a.y {
                edges.push((b, a));
            }
        }
    }
    levels.sort_by(f64::total_cmp);
    levels.dedup();
    let mut out = Vec::new();
    for w in levels.windows(2) {
        let (y0, y1) = (w[0], w[1]);
        let middle = (y0 + y1) / 2.0;
        // Where each edge that crosses the strip is at its bottom, its middle and its top. (An
        // end of the edge is taken as it is, so that strips meet without a gap.)
        let mut crossing: Vec<[f64; 3]> = edges
            .iter()
            .filter(|(lo, hi)| lo.y <= y0 && hi.y >= y1)
            .map(|(lo, hi)| {
                let at = |y: f64| {
                    if y == lo.y {
                        lo.x
                    } else if y == hi.y {
                        hi.x
                    } else {
                        lo.x + (y - lo.y) / (hi.y - lo.y) * (hi.x - lo.x)
                    }
                };
                [at(y0), at(middle), at(y1)]
            })
            .collect();
        crossing.sort_by(|a, b| a[1].total_cmp(&b[1]));
        for pair in crossing.as_chunks::<2>().0 {
            let [left, right] = pair;
            let (a, b, c, d) = (Vec2::new(left[0], y0), Vec2::new(right[0], y0), Vec2::new(right[2], y1), Vec2::new(left[2], y1));
            if b.x > a.x {
                out.push([a, b, c]);
            }
            if c.x > d.x {
                out.push([a, c, d]);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::f64::consts::PI;

    fn filled(tris: &[[Vec2; 3]]) -> f64 {
        tris.iter().map(|t| (t[1] - t[0]).cross(t[2] - t[0]) / 2.0).sum()
    }

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Vec<Vec2> {
        vec![Vec2::new(x, y), Vec2::new(x + w, y), Vec2::new(x + w, y + h), Vec2::new(x, y + h)]
    }

    fn ring(c: Vec2, r: f64, n: usize) -> Vec<Vec2> {
        (0..n).map(|i| Vec2::polar(c, r, 2.0 * PI * i as f64 / n as f64)).collect()
    }

    #[test]
    fn outlines_with_holes_are_filled_exactly() {
        // A square, given either way round.
        let sq = rect(0.0, 0.0, 10.0, 10.0);
        assert!((filled(&triangulate(&sq, &[])) - 100.0).abs() < 1e-9);
        let back: Vec<Vec2> = sq.iter().rev().copied().collect();
        assert!((filled(&triangulate(&back, &[])) - 100.0).abs() < 1e-9);
        // A cross (not convex), and a triangle (its sides slope).
        let cross: Vec<Vec2> = [(4, 0), (8, 0), (8, 4), (12, 4), (12, 8), (8, 8), (8, 12), (4, 12), (4, 8), (0, 8), (0, 4), (4, 4)]
            .iter()
            .map(|&(x, y)| Vec2::new(f64::from(x), f64::from(y)))
            .collect();
        assert!((filled(&triangulate(&cross, &[])) - 80.0).abs() < 1e-9);
        let tri = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 2.0), Vec2::new(3.0, 9.0)];
        assert!((filled(&triangulate(&tri, &[])) - 42.0).abs() < 1e-9, "{}", filled(&triangulate(&tri, &[])));
        // A plate with three holes: two squares side by side and a round one, in any order.
        let plate = rect(0.0, 0.0, 60.0, 40.0);
        let holes = vec![rect(10.0, 10.0, 8.0, 8.0), ring(Vec2::new(45.0, 28.0), 6.0, 48), rect(30.0, 10.0, 8.0, 8.0)];
        let round = 0.5 * 36.0 * 48.0 * (2.0 * PI / 48.0).sin();
        let want = 2400.0 - 128.0 - round;
        assert!((filled(&triangulate(&plate, &holes)) - want).abs() < 1e-6, "{} {want}", filled(&triangulate(&plate, &holes)));
        let other: Vec<Vec<Vec2>> = holes.iter().rev().map(|h| h.iter().rev().copied().collect()).collect();
        assert!((filled(&triangulate(&plate, &other)) - want).abs() < 1e-6);
        // Every triangle counter-clockwise, and none in a hole.
        for t in triangulate(&plate, &holes) {
            assert!((t[1] - t[0]).cross(t[2] - t[0]) > 0.0);
            let mid = (t[0] + t[1] + t[2]) * (1.0 / 3.0);
            assert!(!(mid.x > 10.0 && mid.x < 18.0 && mid.y > 10.0 && mid.y < 18.0), "{mid:?}");
            assert!(mid.dist(Vec2::new(45.0, 28.0)) > 5.9, "{mid:?}");
        }
    }

    #[test]
    fn holes_that_touch_the_outline_or_each_other_are_left_open() {
        // The floor of a tray with a square relief at one corner: an L round (0, 0) with a
        // notch 4 square that touches the outline at that point.
        let outline: Vec<Vec2> =
            [(0.0, 0.0), (0.0, -5.0), (20.0, -5.0), (20.0, 20.0), (-5.0, 20.0), (-5.0, 0.0)].iter().map(|&(x, y)| Vec2::new(x, y)).collect();
        let notch = rect(0.0, 0.0, 4.0, 4.0);
        let t = triangulate(&outline, std::slice::from_ref(&notch));
        assert!((filled(&t) - (600.0 - 16.0)).abs() < 1e-9, "{}", filled(&t));
        // Many holes at once, two of them touching corner to corner.
        let plate = rect(0.0, 0.0, 100.0, 100.0);
        let mut holes: Vec<Vec<Vec2>> =
            (0..5).flat_map(|i| (0..5).map(move |j| ring(Vec2::new(10.0 + 20.0 * f64::from(i), 10.0 + 20.0 * f64::from(j)), 4.0, 24))).collect();
        holes.push(rect(40.0, 40.0, 3.0, 3.0));
        holes.push(rect(43.0, 43.0, 3.0, 3.0));
        let round = 0.5 * 16.0 * 24.0 * (2.0 * PI / 24.0).sin();
        let t = triangulate(&plate, &holes);
        assert!((filled(&t) - (10_000.0 - 25.0 * round - 18.0)).abs() < 1e-6, "{}", filled(&t));
        // Nothing to fill: nothing.
        assert!(triangulate(&[], &[]).is_empty() && triangulate(&[Vec2::ZERO, Vec2::X], &[]).is_empty());
    }
}
