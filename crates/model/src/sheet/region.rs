//! Regions of the flat sheet bounded by lines and arcs, and what two of them have in common:
//! the part of a profile that lies on one face, or on one bend, of the sheet laid flat.
//!
//! A region is its boundary: closed loops of [`Piece`]s, and a point is inside when it is within
//! an odd number of them (so a loop inside another is a hole).

use std::f64::consts::{PI, TAU};

use tenon_geom::Vec2;
use tenon_kernel::{Curve2, Loop};

use super::flat::{FlatCurve, FlatLoop};
use super::outline::{self, Piece};

/// Two points this close are one (mm).
const MEET: f64 = 1e-7;
/// The ends of two curves this close join (mm): a sketch's arc ends where its line starts only
/// as closely as the sketch was solved.
const JOIN: f64 = 1e-6;
/// How far to either side of a boundary two regions share each is looked for (mm).
const BESIDE: f64 = 1e-5;
/// A loop with less inside it than this is nothing (mm²).
const NOTHING: f64 = 1e-9;

/// One connected region: its outer boundary and its holes, each a closed chain in order.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub outer: Vec<Piece>,
    pub holes: Vec<Vec<Piece>>,
}

impl Shape {
    pub fn area(&self) -> f64 {
        outline::area(&self.outer) - self.holes.iter().map(|h| outline::area(h)).sum::<f64>()
    }
}

fn length(p: &Piece) -> f64 {
    match p.arc {
        None => p.a.dist(p.b),
        Some((c, r)) => r * p.sweep(c),
    }
}

/// How far along `piece` a point on it is: 0 at its start, 1 at its end.
fn along(piece: &Piece, p: Vec2) -> f64 {
    match piece.arc {
        None => {
            let d = piece.b - piece.a;
            (p - piece.a).dot(d) / d.len2()
        }
        Some((c, _)) => {
            let sweep = piece.sweep(c);
            let mut turned = ((p - c).angle() - (piece.a - c).angle()).rem_euclid(TAU);
            // Just before the start is the start, not most of a turn on from it.
            if turned > sweep + (TAU - sweep) / 2.0 {
                turned -= TAU;
            }
            turned / sweep
        }
    }
}

/// Whether `p` is on the piece.
fn on(piece: &Piece, p: Vec2) -> bool {
    let long = length(piece);
    if long <= MEET {
        return p.dist(piece.a) <= MEET;
    }
    match piece.arc {
        None => p.dist(piece.a + (piece.b - piece.a) * along(piece, p).clamp(0.0, 1.0)) <= MEET,
        Some((c, r)) => (p.dist(c) - r).abs() <= MEET && (-MEET / long..=1.0 + MEET / long).contains(&along(piece, p)),
    }
}

/// Where the line through `a` and `b` meets the circle about `c`: two points, the one where it
/// touches, or none.
fn line_circle(a: Vec2, b: Vec2, c: Vec2, r: f64) -> Vec<Vec2> {
    let long = a.dist(b);
    if long <= MEET {
        return Vec::new();
    }
    let u = (b - a) / long;
    let foot = a + u * (c - a).dot(u);
    let off = foot.dist(c);
    if off > r + MEET {
        Vec::new()
    } else if off >= r - MEET {
        vec![foot]
    } else {
        let half = (r * r - off * off).sqrt();
        vec![foot - u * half, foot + u * half]
    }
}

fn circle_circle(c1: Vec2, r1: f64, c2: Vec2, r2: f64) -> Vec<Vec2> {
    let apart = c1.dist(c2);
    if apart <= MEET || apart > r1 + r2 + MEET || apart < (r1 - r2).abs() - MEET {
        return Vec::new();
    }
    let u = (c2 - c1) / apart;
    let to_chord = (r1 * r1 - r2 * r2 + apart * apart) / (2.0 * apart);
    let foot = c1 + u * to_chord;
    let half2 = r1 * r1 - to_chord * to_chord;
    if half2 <= 2.0 * r1.min(r2) * MEET {
        vec![foot]
    } else {
        let half = half2.sqrt();
        vec![foot + u.perp() * half, foot - u.perp() * half]
    }
}

/// Where two pieces cross or touch. (Where they run along one another they meet at the ends of
/// one or the other: see [`common`].)
fn crossings(p: &Piece, q: &Piece) -> Vec<Vec2> {
    let mut found = match (p.arc, q.arc) {
        (None, None) => {
            let (d, e) = (p.b - p.a, q.b - q.a);
            let across = d.cross(e);
            if across.abs() <= 1e-12 * d.len() * e.len() { Vec::new() } else { vec![p.a + d * ((q.a - p.a).cross(e) / across)] }
        }
        (None, Some((c, r))) => line_circle(p.a, p.b, c, r),
        (Some((c, r)), None) => line_circle(q.a, q.b, c, r),
        (Some((c1, r1)), Some((c2, r2))) => circle_circle(c1, r1, c2, r2),
    };
    found.retain(|x| on(p, *x) && on(q, *x));
    found
}

/// Whether `p` is inside the region whose boundary `loops` are: within an odd number of them.
/// (Not for a point on the boundary itself.)
pub fn inside(loops: &[Vec<Piece>], p: Vec2) -> bool {
    // A ray to the right of `p` crosses a stretch of boundary that goes only up or only down
    // when the stretch's two ends are on either side of it, and the stretch is to the right.
    let mut odd = false;
    let mut stretch = |from: Vec2, to: Vec2, x: &dyn Fn() -> f64| {
        if (from.y > p.y) != (to.y > p.y) && x() > p.x {
            odd = !odd;
        }
    };
    for piece in loops.iter().flatten() {
        match piece.arc {
            None => {
                let (a, b) = (piece.a, piece.b);
                stretch(a, b, &|| a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x));
            }
            Some((c, r)) => {
                // An arc goes only up or only down between the top and the bottom of its circle.
                let (start, sweep) = ((piece.a - c).angle(), piece.sweep(c));
                let mut stops: Vec<(f64, Vec2)> = vec![(0.0, piece.a), (sweep, piece.b)];
                for (angle, at) in [(PI / 2.0, c + Vec2::new(0.0, r)), (-PI / 2.0, c - Vec2::new(0.0, r))] {
                    let turned = (angle - start).rem_euclid(TAU);
                    if turned > 0.0 && turned < sweep {
                        stops.push((turned, at));
                    }
                }
                stops.sort_by(|x, y| x.0.total_cmp(&y.0));
                for w in stops.windows(2) {
                    let right = (start + (w[0].0 + w[1].0) / 2.0).cos() > 0.0;
                    let reach = (r * r - (p.y - c.y) * (p.y - c.y)).max(0.0).sqrt();
                    stretch(w[0].1, w[1].1, &|| if right { c.x + reach } else { c.x - reach });
                }
            }
        }
    }
    odd
}

/// Each piece divided at the points given for it.
fn divided(pieces: &[Piece], cuts: &[Vec<Vec2>]) -> Vec<Piece> {
    let mut out = Vec::new();
    for (piece, at) in pieces.iter().zip(cuts) {
        let long = length(piece);
        if long <= MEET {
            continue;
        }
        let mut stops: Vec<(f64, Vec2)> =
            at.iter().map(|x| (along(piece, *x), *x)).filter(|(t, _)| t * long > MEET && (1.0 - t) * long > MEET).collect();
        stops.sort_by(|x, y| x.0.total_cmp(&y.0));
        stops.dedup_by(|x, y| (x.0 - y.0).abs() * long <= MEET);
        let mut from = piece.a;
        for (_, to) in stops.into_iter().chain([(1.0, piece.b)]) {
            out.push(Piece { a: from, b: to, arc: piece.arc, tag: piece.tag });
            from = to;
        }
    }
    out
}

/// The pieces as closed chains, each piece starting where the one before ended (an arc may be
/// met at either end).
fn closed(mut left: Vec<Piece>) -> Result<Vec<Vec<Piece>>, String> {
    let mut loops = Vec::new();
    while let Some(first) = left.pop() {
        let (home, mut here) = (first.a, first.b);
        let mut chain = vec![first];
        while here.dist(home) > JOIN || chain.len() < 2 {
            let nearest = left
                .iter()
                .enumerate()
                .map(|(i, p)| (i, p.a.dist(here).min(p.b.dist(here))))
                .filter(|(_, d)| *d <= JOIN)
                .min_by(|x, y| x.1.total_cmp(&y.1));
            let Some((i, _)) = nearest else {
                return Err("an outline that does not close".into());
            };
            let mut next = left.swap_remove(i);
            // A line is made to start exactly where the last piece ended (or that one, a line,
            // to end where this arc starts), so that the chain has no gaps for whoever reads it.
            let first_end = next.a.dist(here) <= next.b.dist(here);
            if next.arc.is_none() {
                *(if first_end { &mut next.a } else { &mut next.b }) = here;
            } else if let Some(last) = chain.last_mut().filter(|p| p.arc.is_none()) {
                let at = if first_end { next.a } else { next.b };
                *(if last.b.dist(here) <= last.a.dist(here) { &mut last.b } else { &mut last.a }) = at;
            }
            here = if first_end { next.b } else { next.a };
            chain.push(next);
        }
        let n = chain.len();
        if chain[n - 1].arc.is_none() {
            let last = &mut chain[n - 1];
            *(if last.b.dist(here) <= last.a.dist(here) { &mut last.b } else { &mut last.a }) = home;
        } else if chain[0].arc.is_none() {
            chain[0].a = here;
        }
        loops.push(joined(chain));
    }
    Ok(loops)
}

/// Two pieces that follow one another as one, when they are parts of one line or of one circle
/// (and of one curve of the profile: the same tag) and do not make a whole circle between them.
fn as_one(p: &Piece, q: &Piece) -> Option<Piece> {
    if p.tag != q.tag {
        return None;
    }
    match (p.arc, q.arc) {
        (None, None) => {
            // From the end of `p` that is not `q`'s to the end of `q` that is not `p`'s.
            let at = if p.b.dist(q.a).min(p.b.dist(q.b)) <= p.a.dist(q.a).min(p.a.dist(q.b)) { p.b } else { p.a };
            let (from, to) = (if at == p.b { p.a } else { p.b }, if q.a.dist(at) <= q.b.dist(at) { q.b } else { q.a });
            let (d, e) = (at - from, to - at);
            (d.cross(e).abs() <= 1e-12 * d.len() * e.len() && d.dot(e) > 0.0).then(|| Piece::line(from, to, p.tag))
        }
        (Some((c, r)), Some((d, s))) if c.dist(d) <= MEET && (r - s).abs() <= MEET => {
            let (a, b) = if p.b.dist(q.a) <= JOIN {
                (p.a, q.b)
            } else if q.b.dist(p.a) <= JOIN {
                (q.a, p.b)
            } else {
                return None;
            };
            (a.dist(b) > JOIN).then_some(Piece { a, b, arc: p.arc, tag: p.tag })
        }
        _ => None,
    }
}

/// A closed chain with the pieces that are parts of one line or one circle made one again.
fn joined(mut chain: Vec<Piece>) -> Vec<Piece> {
    let mut i = 0;
    while chain.len() > 2 && i < chain.len() {
        let next = (i + 1) % chain.len();
        match as_one(&chain[i], &chain[next]) {
            Some(one) => {
                chain[i] = one;
                chain.remove(next);
                i = 0;
            }
            None => i += 1,
        }
    }
    chain
}

/// Loops as regions: a loop inside another is its hole, a loop inside that a region again.
fn nested(loops: Vec<Vec<Piece>>) -> Vec<Shape> {
    let loops: Vec<Vec<Piece>> = loops.into_iter().filter(|l| outline::area(l) > NOTHING).collect();
    let probe: Vec<Vec2> = loops.iter().map(|l| l[0].mid()).collect();
    let within = |i: usize, j: usize| i != j && inside(std::slice::from_ref(&loops[j]), probe[i]);
    let depth: Vec<usize> = (0..loops.len()).map(|i| (0..loops.len()).filter(|&j| within(i, j)).count()).collect();
    (0..loops.len())
        .filter(|&i| depth[i].is_multiple_of(2))
        .map(|i| Shape {
            outer: loops[i].clone(),
            holes: (0..loops.len()).filter(|&j| depth[j] == depth[i] + 1 && within(j, i)).map(|j| loops[j].clone()).collect(),
        })
        .collect()
}

/// What two regions have in common, each given by the loops of its boundary. Every piece of the
/// result keeps the tag of the piece it is part of; where the two share a stretch of boundary it
/// is `a`'s.
pub fn common(a: &[Vec<Piece>], b: &[Vec<Piece>]) -> Result<Vec<Shape>, String> {
    let all = |loops: &[Vec<Piece>]| -> Vec<Piece> { loops.iter().flatten().copied().collect() };
    let (pa, pb) = (all(a), all(b));
    // Where each piece is to be divided: where it crosses the other region's boundary, and
    // where a piece of that ends on it (the two may run along one another from there).
    let (mut cuts_a, mut cuts_b) = (vec![Vec::new(); pa.len()], vec![Vec::new(); pb.len()]);
    for (i, p) in pa.iter().enumerate() {
        for (j, q) in pb.iter().enumerate() {
            for x in crossings(p, q) {
                cuts_a[i].push(x);
                cuts_b[j].push(x);
            }
            cuts_a[i].extend([q.a, q.b].into_iter().filter(|e| on(p, *e)));
            cuts_b[j].extend([p.a, p.b].into_iter().filter(|e| on(q, *e)));
        }
    }
    let mut kept: Vec<Piece> = Vec::new();
    for s in divided(&pa, &cuts_a) {
        let m = s.mid();
        if pb.iter().any(|q| on(q, m)) {
            // Along the other's boundary: the boundary of what they have in common when both
            // lie to the same side of it.
            let normal = match s.arc {
                None => (s.b - s.a).perp().normalized(),
                Some((c, _)) => (m - c).normalized(),
            };
            if [m + normal * BESIDE, m - normal * BESIDE].iter().all(|at| inside(a, *at) == inside(b, *at)) {
                kept.push(s);
            }
        } else if inside(b, m) {
            kept.push(s);
        }
    }
    for s in divided(&pb, &cuts_b) {
        let m = s.mid();
        if !pa.iter().any(|q| on(q, m)) && inside(a, m) {
            kept.push(s);
        }
    }
    Ok(nested(closed(kept)?))
}

/// The lowest and highest corner of the box round some loops.
pub fn bounds(loops: &[Vec<Piece>]) -> (Vec2, Vec2) {
    let mut lo = Vec2::new(f64::INFINITY, f64::INFINITY);
    let mut hi = Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in loops.iter().flatten() {
        let (a, b) = match p.arc {
            // (The whole circle's box: more than the arc's, which is all that is asked.)
            Some((c, r)) => (c - Vec2::new(r, r), c + Vec2::new(r, r)),
            None => (p.a.min(p.b), p.a.max(p.b)),
        };
        (lo, hi) = (lo.min(a), hi.max(b));
    }
    (lo, hi)
}

/// A loop of a profile as pieces: a circle as two half circles. A spline has none.
pub fn of_loop(l: &Loop) -> Result<Vec<Piece>, String> {
    let mut out = Vec::new();
    for c in &l.curves {
        match &c.curve {
            Curve2::Line { start, end } => out.push(Piece::line(*start, *end, c.tag)),
            Curve2::Arc { center, radius, start_angle, end_angle } => out.push(Piece {
                a: Vec2::polar(*center, *radius, *start_angle),
                b: Vec2::polar(*center, *radius, *end_angle),
                arc: Some((*center, *radius)),
                tag: c.tag,
            }),
            Curve2::Circle { center, radius } => out.extend(circle(*center, *radius, c.tag)),
            Curve2::BSpline { .. } => return Err("a spline".into()),
        }
    }
    Ok(out)
}

fn circle(center: Vec2, radius: f64, tag: u64) -> [Piece; 2] {
    let (a, b) = (center + Vec2::new(radius, 0.0), center - Vec2::new(radius, 0.0));
    [Piece { a, b, arc: Some((center, radius)), tag }, Piece { a: b, b: a, arc: Some((center, radius)), tag }]
}

/// A loop of the flat sheet as pieces, all tagged `tag`: a curve that is neither a line nor an
/// arc as the points it was given by.
pub fn of_flat(l: &FlatLoop, tag: u64) -> Vec<Piece> {
    let mut out = Vec::new();
    for c in &l.curves {
        match c {
            FlatCurve::Line { start, end } => out.push(Piece::line(*start, *end, tag)),
            FlatCurve::Arc { center, radius, start, end, ccw } => {
                let (a, b) = if *ccw { (*start, *end) } else { (*end, *start) };
                out.push(Piece { a, b, arc: Some((*center, *radius)), tag });
            }
            FlatCurve::Circle { center, radius } => out.extend(circle(*center, *radius, tag)),
            FlatCurve::Polyline { points } => out.extend(points.windows(2).map(|w| Piece::line(w[0], w[1], tag))),
        }
    }
    out.retain(|p| length(p) > MEET);
    out
}

/// A closed chain as the kernel takes it. The two halves of a whole circle (one curve of the
/// profile: one tag) are the circle again, so that a round hole has one wall.
pub fn to_loop(pieces: &[Piece]) -> Loop {
    if let [p, q] = pieces
        && let (Some((c, r)), Some((d, s))) = (p.arc, q.arc)
        && p.tag == q.tag
        && c.dist(d) <= MEET
        && (r - s).abs() <= MEET
        && p.a.dist(q.b) <= JOIN
        && p.b.dist(q.a) <= JOIN
    {
        return Loop { curves: vec![tenon_kernel::TaggedCurve2 { tag: p.tag, curve: Curve2::Circle { center: c, radius: r } }] };
    }
    outline::to_loop(pieces)
}

/// A closed chain with its pieces moved by `to`: a rigid move of the plane, turned over when
/// `mirrored`.
pub fn moved(pieces: &[Piece], to: impl Fn(Vec2) -> Vec2, mirrored: bool) -> Vec<Piece> {
    pieces
        .iter()
        .map(|p| {
            let (a, b) = (to(p.a), to(p.b));
            // An arc runs counter-clockwise: turned over, from its other end.
            let (a, b) = if mirrored && p.arc.is_some() { (b, a) } else { (a, b) };
            Piece { a, b, arc: p.arc.map(|(c, r)| (to(c), r)), tag: p.tag }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::sheet::outline::{area, polygon};

    fn rect(x: f64, y: f64, w: f64, h: f64, tag: u64) -> Vec<Piece> {
        polygon(&[(Vec2::new(x, y), tag), (Vec2::new(x + w, y), tag + 1), (Vec2::new(x + w, y + h), tag + 2), (Vec2::new(x, y + h), tag + 3)])
    }

    fn disc(x: f64, y: f64, r: f64, tag: u64) -> Vec<Piece> {
        circle(Vec2::new(x, y), r, tag).to_vec()
    }

    /// The area of a disc of radius `r` beyond a line `d` from its centre.
    fn cap(r: f64, d: f64) -> f64 {
        r * r * (d / r).acos() - d * (r * r - d * d).sqrt()
    }

    #[test]
    fn a_point_is_inside_a_region_or_in_a_hole_of_it() {
        let ring = [disc(0.0, 0.0, 5.0, 1), disc(0.0, 0.0, 2.0, 2)];
        for (x, y, is) in [(3.0, 0.0, true), (0.0, 0.0, false), (0.0, 3.5, true), (-4.9, 0.3, true), (5.1, 0.0, false), (3.0, 4.1, false)] {
            assert_eq!(inside(&ring, Vec2::new(x, y)), is, "{x} {y}");
        }
        // Level with a corner, and with the top of an arc: counted once.
        let plate = [rect(0.0, 0.0, 10.0, 6.0, 1)];
        assert!(inside(&plate, Vec2::new(5.0, 3.0)) && !inside(&plate, Vec2::new(-1.0, 6.0)) && !inside(&plate, Vec2::new(-1.0, 0.0)));
        assert!(!inside(&ring, Vec2::new(-6.0, 5.0)) && !inside(&ring, Vec2::new(-6.0, -5.0)) && inside(&ring, Vec2::new(-3.0, 2.0)));
    }

    #[test]
    fn what_two_regions_have_in_common_is_bounded_by_pieces_of_both() {
        // Two rectangles that overlap in a 3 x 2 one: two sides from each.
        let got = common(&[rect(0.0, 0.0, 10.0, 6.0, 10)], &[rect(7.0, 4.0, 10.0, 6.0, 20)]).unwrap();
        assert!(got.len() == 1 && got[0].holes.is_empty() && (got[0].area() - 6.0).abs() < 1e-12, "{got:?}");
        let tags = |s: &Shape| -> Vec<u64> {
            let mut t: Vec<u64> = s.outer.iter().map(|p| p.tag).collect();
            t.sort_unstable();
            t
        };
        assert_eq!(tags(&got[0]), [11, 12, 20, 23]);

        // A disc of radius 5 across a strip 6 wide: two arcs of the disc, two lines of the strip.
        let got = common(&[disc(0.0, 0.0, 5.0, 1)], &[rect(-3.0, -20.0, 6.0, 40.0, 20)]).unwrap();
        let whole = 25.0 * PI - 2.0 * cap(5.0, 3.0);
        assert!(got.len() == 1 && (got[0].area() - whole).abs() < 1e-12, "{} {whole}", got[0].area());
        assert_eq!((got[0].outer.iter().filter(|p| p.arc.is_some()).count(), got[0].outer.iter().filter(|p| p.arc.is_none()).count()), (2, 2));
        // The rest of the disc is the two caps: two regions, each an arc and a line.
        let beside = common(&[disc(0.0, 0.0, 5.0, 1)], &[rect(3.0, -20.0, 6.0, 40.0, 20), rect(-9.0, -20.0, 6.0, 40.0, 30)]).unwrap();
        assert!(beside.len() == 2 && beside.iter().all(|s| s.outer.len() == 2 && (s.area() - cap(5.0, 3.0)).abs() < 1e-12), "{beside:?}");

        // Two discs: a lens.
        let got = common(&[disc(0.0, 0.0, 5.0, 1)], &[disc(6.0, 0.0, 5.0, 2)]).unwrap();
        assert!(got.len() == 1 && got[0].outer.len() == 2 && (got[0].area() - 2.0 * cap(5.0, 3.0)).abs() < 1e-12, "{got:?}");

        // One inside the other is the inner one, whichever is given first; apart, nothing.
        let (small, big) = ([disc(4.0, 3.0, 1.0, 1)], [rect(0.0, 0.0, 10.0, 6.0, 10)]);
        for got in [common(&small, &big).unwrap(), common(&big, &small).unwrap()] {
            assert!(got.len() == 1 && (got[0].area() - PI).abs() < 1e-12 && got[0].outer.iter().all(|p| p.tag == 1), "{got:?}");
        }
        assert!(common(&small, &[rect(20.0, 0.0, 5.0, 5.0, 10)]).unwrap().is_empty());
    }

    #[test]
    fn boundaries_that_run_along_one_another_are_counted_once_or_not_at_all() {
        // A rectangle drawn from the very edge of a strip, and as tall as it: the part on the
        // strip, with the shared sides once.
        let got = common(&[rect(0.0, 0.0, 2.0, 1.0, 10)], &[rect(1.0, 0.0, 2.0, 1.0, 20)]).unwrap();
        assert!(got.len() == 1 && got[0].outer.len() == 4 && (got[0].area() - 1.0).abs() < 1e-12, "{got:?}");
        // The same region twice is itself.
        let got = common(&[rect(0.0, 0.0, 2.0, 1.0, 10)], &[rect(0.0, 0.0, 2.0, 1.0, 20)]).unwrap();
        assert!(got.len() == 1 && (got[0].area() - 2.0).abs() < 1e-12 && got[0].outer.iter().all(|p| p.tag < 20), "{got:?}");
        // Side by side, sharing a side and nothing else: nothing in common. Corner to corner too.
        assert!(common(&[rect(0.0, 0.0, 2.0, 1.0, 10)], &[rect(2.0, 0.0, 2.0, 1.0, 20)]).unwrap().is_empty());
        assert!(common(&[rect(0.0, 0.0, 2.0, 1.0, 10)], &[rect(2.0, 1.0, 2.0, 1.0, 20)]).unwrap().is_empty());
        // A disc that touches a line from inside, and one that touches it from outside.
        let got = common(&[disc(0.0, 2.0, 2.0, 1)], &[rect(-5.0, 0.0, 10.0, 6.0, 20)]).unwrap();
        assert!(got.len() == 1 && (got[0].area() - 4.0 * PI).abs() < 1e-9, "{got:?}");
        assert!(common(&[disc(0.0, -2.0, 2.0, 1)], &[rect(-5.0, 0.0, 10.0, 6.0, 20)]).unwrap().is_empty());
        // An arc of the same circle: a half disc on a disc is the half disc.
        let half = vec![circle(Vec2::ZERO, 3.0, 1)[0], Piece::line(Vec2::new(-3.0, 0.0), Vec2::new(3.0, 0.0), 2)];
        let got = common(std::slice::from_ref(&half), &[disc(0.0, 0.0, 3.0, 9)]).unwrap();
        assert!(got.len() == 1 && (got[0].area() - 4.5 * PI).abs() < 1e-12 && got[0].outer.iter().all(|p| p.tag < 9), "{got:?}");
    }

    #[test]
    fn a_hole_is_kept_where_it_is_whole_and_opens_the_outline_where_it_is_cut() {
        let ring = [disc(0.0, 0.0, 5.0, 1), disc(0.0, 0.0, 2.0, 2)];
        // All of the ring: an outline with its hole.
        let got = common(&ring, &[rect(-10.0, -10.0, 20.0, 20.0, 20)]).unwrap();
        assert!(got.len() == 1 && got[0].holes.len() == 1 && (got[0].area() - 21.0 * PI).abs() < 1e-12, "{got:?}");
        // The right half of it: one outline, a C.
        let got = common(&ring, &[rect(0.0, -10.0, 20.0, 20.0, 20)]).unwrap();
        assert!(got.len() == 1 && got[0].holes.is_empty() && (got[0].area() - 10.5 * PI).abs() < 1e-12, "{got:?}");
        assert_eq!(got[0].outer.len(), 4, "{got:?}");
        // A strip through the hole: two pieces of ring.
        let got = common(&ring, &[rect(-1.0, -10.0, 2.0, 20.0, 20)]).unwrap();
        assert!(got.len() == 2 && got.iter().all(|s| s.holes.is_empty() && s.outer.len() == 4), "{got:?}");
        assert!(
            (got[0].area() - got[1].area()).abs() < 1e-12
                && (area(&got[0].outer) - (cap(5.0, -1.0) - cap(5.0, 1.0) - cap(2.0, -1.0) + cap(2.0, 1.0)) / 2.0).abs() < 1e-12
        );
    }

    #[test]
    fn profiles_and_flat_loops_become_pieces_and_pieces_are_moved() {
        use tenon_kernel::TaggedCurve2;
        let l = Loop {
            curves: vec![
                TaggedCurve2 { tag: 1, curve: Curve2::Line { start: Vec2::new(-3.0, 0.0), end: Vec2::new(3.0, 0.0) } },
                TaggedCurve2 { tag: 2, curve: Curve2::Arc { center: Vec2::ZERO, radius: 3.0, start_angle: 0.0, end_angle: PI } },
            ],
        };
        let pieces = of_loop(&l).unwrap();
        assert!((area(&pieces) - 4.5 * PI).abs() < 1e-12 && pieces[1].a.near(Vec2::new(3.0, 0.0), 1e-12), "{pieces:?}");
        let whole = Loop { curves: vec![TaggedCurve2 { tag: 7, curve: Curve2::Circle { center: Vec2::new(1.0, 1.0), radius: 2.0 } }] };
        assert!((area(&of_loop(&whole).unwrap()) - 4.0 * PI).abs() < 1e-12);
        let spline = Loop { curves: vec![TaggedCurve2 { tag: 7, curve: Curve2::BSpline { poles: vec![Vec2::ZERO; 4], degree: 3 } }] };
        assert!(of_loop(&spline).is_err());

        let flat = FlatLoop {
            curves: vec![
                FlatCurve::Line { start: Vec2::new(-3.0, 0.0), end: Vec2::new(3.0, 0.0) },
                FlatCurve::Arc { center: Vec2::ZERO, radius: 3.0, start: Vec2::new(-3.0, 0.0), end: Vec2::new(3.0, 0.0), ccw: false },
            ],
        };
        assert!((area(&of_flat(&flat, 5)) - 4.5 * PI).abs() < 1e-12);

        // Turned over about the Y axis and moved: the half disc still has its area and its arc
        // still runs counter-clockwise, now from the other end.
        let over = moved(&pieces, |p| Vec2::new(10.0 - p.x, p.y + 1.0), true);
        assert!((area(&over) - 4.5 * PI).abs() < 1e-12, "{over:?}");
        assert!(over[1].a.near(Vec2::new(13.0, 1.0), 1e-12) && over[1].mid().near(Vec2::new(10.0, 4.0), 1e-12), "{over:?}");
        assert_eq!(bounds(&[over]), (Vec2::new(7.0, -2.0), Vec2::new(13.0, 4.0)));

        // The two halves of a circle go to the kernel as the circle; two arcs of different
        // circles (a lens) as the two arcs they are.
        let round = to_loop(&circle(Vec2::new(1.0, 1.0), 2.0, 7));
        assert!(matches!(round.curves.as_slice(), [c] if c.tag == 7 && c.curve == Curve2::Circle { center: Vec2::new(1.0, 1.0), radius: 2.0 }), "{round:?}");
        let lens = common(&[circle(Vec2::ZERO, 5.0, 1).to_vec()], &[circle(Vec2::new(6.0, 0.0), 5.0, 2).to_vec()]).unwrap();
        assert_eq!(to_loop(&lens[0].outer).curves.len(), 2);
    }
}
