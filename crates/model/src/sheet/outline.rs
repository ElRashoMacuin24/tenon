//! Outlines in the flat: the pieces of a flange (its bend, its wall) as closed outlines of lines
//! and arcs, and what a corner relief takes out of them.

use std::f64::consts::TAU;

use tenon_geom::Vec2;
use tenon_kernel::{Curve2, Loop, TaggedCurve2};

use super::flat::within;

/// Lengths shorter than this are nothing (mm).
const EPS: f64 = 1e-9;
/// Two ends this close are one point (mm).
const MEET: f64 = 1e-7;

/// One curve of an outline: the line from `a` to `b`, or an arc between them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    pub a: Vec2,
    pub b: Vec2,
    /// An arc's centre and radius; it runs counter-clockwise from `a` to `b`.
    pub arc: Option<(Vec2, f64)>,
    /// What the face this piece makes is named after.
    pub tag: u64,
}

impl Piece {
    pub fn line(a: Vec2, b: Vec2, tag: u64) -> Piece {
        Piece { a, b, arc: None, tag }
    }

    /// How far the arc turns.
    fn sweep(&self, center: Vec2) -> f64 {
        let d = ((self.b - center).angle() - (self.a - center).angle()).rem_euclid(TAU);
        if d < 1e-12 { TAU } else { d }
    }

    /// The point half way along.
    pub fn mid(&self) -> Vec2 {
        match self.arc {
            None => (self.a + self.b) / 2.0,
            Some((c, r)) => Vec2::polar(c, r, (self.a - c).angle() + self.sweep(c) / 2.0),
        }
    }

    /// Points along it from `a`, without `b`: the outline as a polygon is all of these.
    fn points(&self) -> Vec<Vec2> {
        match self.arc {
            None => vec![self.a],
            Some((c, r)) => {
                let (from, sweep) = ((self.a - c).angle(), self.sweep(c));
                let n = ((sweep / 0.05).ceil() as usize).max(2);
                (0..n).map(|i| if i == 0 { self.a } else { Vec2::polar(c, r, from + sweep * i as f64 / n as f64) }).collect()
            }
        }
    }

    pub fn moved(&self, by: Vec2) -> Piece {
        Piece { a: self.a + by, b: self.b + by, arc: self.arc.map(|(c, r)| (c + by, r)), tag: self.tag }
    }

    pub fn curve(&self) -> TaggedCurve2 {
        let curve = match self.arc {
            None => Curve2::Line { start: self.a, end: self.b },
            Some((center, radius)) => Curve2::Arc { center, radius, start_angle: (self.a - center).angle(), end_angle: (self.b - center).angle() },
        };
        TaggedCurve2 { tag: self.tag, curve }
    }
}

/// What a corner relief removes, seen in the flat: a disc or a convex polygon.
#[derive(Clone, Debug, PartialEq)]
pub enum Cutter {
    Disc { center: Vec2, radius: f64 },
    Polygon(Vec<Vec2>),
}

impl Cutter {
    /// Whether `p` is inside (on its edge is not).
    pub fn covers(&self, p: Vec2) -> bool {
        match self {
            Cutter::Disc { center, radius } => p.dist(*center) < radius - EPS,
            Cutter::Polygon(corners) => {
                let n = corners.len();
                let turn: f64 = (0..n).map(|i| corners[i].cross(corners[(i + 1) % n])).sum();
                (0..n).all(|i| {
                    let (v, e) = (corners[i], corners[(i + 1) % n] - corners[i]);
                    e.cross(p - v) * turn.signum() > EPS * e.len()
                })
            }
        }
    }

    pub fn moved(&self, by: Vec2) -> Cutter {
        match self {
            Cutter::Disc { center, radius } => Cutter::Disc { center: *center + by, radius: *radius },
            Cutter::Polygon(corners) => Cutter::Polygon(corners.iter().map(|c| *c + by).collect()),
        }
    }

    /// Its own outline, every curve tagged `tag`.
    pub fn outline(&self, tag: u64) -> Loop {
        match self {
            Cutter::Disc { center, radius } => {
                Loop { curves: vec![TaggedCurve2 { tag, curve: Curve2::Circle { center: *center, radius: *radius } }] }
            }
            Cutter::Polygon(corners) => {
                let n = corners.len();
                Loop { curves: (0..n).map(|i| Piece::line(corners[i], corners[(i + 1) % n], tag).curve()).collect() }
            }
        }
    }

    /// Where the line from `a` to `b` crosses the cutter's edge, as fractions of the way along.
    fn crossings(&self, a: Vec2, b: Vec2) -> Vec<f64> {
        let d = b - a;
        let on = |t: f64| (-1e-9..=1.0 + 1e-9).contains(&t).then(|| t.clamp(0.0, 1.0));
        let mut out: Vec<f64> = Vec::new();
        match self {
            Cutter::Disc { center, radius } => {
                let (qa, qb, qc) = (d.dot(d), d.dot(a - *center), (a - *center).dot(a - *center) - radius * radius);
                let disc = qb * qb - qa * qc;
                if qa > 0.0 && disc > 0.0 {
                    let root = disc.sqrt();
                    out.extend([(-qb - root) / qa, (-qb + root) / qa].into_iter().filter_map(on));
                }
            }
            Cutter::Polygon(corners) => {
                let n = corners.len();
                for i in 0..n {
                    let (p, e) = (corners[i], corners[(i + 1) % n] - corners[i]);
                    let denom = d.cross(e);
                    if denom.abs() <= 1e-12 * d.len() * e.len() {
                        continue;
                    }
                    let (t, s) = ((p - a).cross(e) / denom, (p - a).cross(d) / denom);
                    if on(s).is_some() {
                        out.extend(on(t));
                    }
                }
            }
        }
        out.sort_by(f64::total_cmp);
        out.dedup_by(|x, y| (*x - *y).abs() * d.len() < MEET);
        out
    }
}

/// The polygon with these corners in order, each side tagged as given with the corner it starts
/// at. Sides of no length are left out.
pub fn polygon(corners: &[(Vec2, u64)]) -> Vec<Piece> {
    let n = corners.len();
    (0..n).map(|i| Piece::line(corners[i].0, corners[(i + 1) % n].0, corners[i].1)).filter(|p| p.a.dist(p.b) > MEET).collect()
}

/// The pieces in order round the outline, each starting where the one before ended (an arc may
/// be met at either end). More than one outline, or one that does not close, is an error.
fn chained(mut pieces: Vec<Piece>) -> Result<Vec<Piece>, &'static str> {
    let Some(first) = pieces.pop() else { return Ok(Vec::new()) };
    let (home, mut here) = (first.a, first.b);
    let mut out = vec![first];
    while here.dist(home) > MEET {
        let Some(i) = pieces.iter().position(|p| p.a.dist(here) <= MEET || p.b.dist(here) <= MEET) else {
            return Err("open");
        };
        let next = pieces.swap_remove(i);
        here = if next.a.dist(here) <= MEET { next.b } else { next.a };
        out.push(next);
    }
    if pieces.is_empty() { Ok(out) } else { Err("apart") }
}

/// `outline` less what `cutter` covers. What the cutter's own edge leaves is tagged `tag`.
/// Nothing left is an empty outline; an outline cut in two is an error.
pub fn cut(outline: &[Piece], cutter: &Cutter, tag: u64) -> Result<Vec<Piece>, String> {
    let region: Vec<Vec2> = outline.iter().flat_map(Piece::points).collect();
    let mut kept: Vec<Piece> = Vec::new();
    // Whether the cutter takes any of the outline itself.
    let mut gone = false;
    // Where the outline crosses the cutter's edge.
    let mut marks: Vec<Vec2> = Vec::new();
    let mut mark = |p: Vec2| {
        if !marks.iter().any(|m| m.dist(p) <= MEET) {
            marks.push(p);
        }
    };
    for p in outline {
        if p.arc.is_some() {
            // What an earlier relief left: this one has no business there.
            if [p.a, p.mid(), p.b].iter().any(|q| cutter.covers(*q)) {
                return Err("The corner reliefs at the two ends of an edge run into each other: make the relief smaller, or the edge longer.".into());
            }
            kept.push(*p);
            continue;
        }
        let d = p.b - p.a;
        let crossings = cutter.crossings(p.a, p.b);
        let mut stops = vec![0.0];
        stops.extend(crossings.iter().copied());
        stops.push(1.0);
        for w in stops.windows(2) {
            let (s, e) = (p.a + d * w[0], p.a + d * w[1]);
            if s.dist(e) <= MEET {
                continue;
            }
            if cutter.covers((s + e) / 2.0) {
                gone = true;
            } else {
                kept.push(Piece::line(s, e, p.tag));
            }
        }
        for t in crossings {
            mark(p.a + d * t);
        }
    }
    if kept.is_empty() {
        return Ok(kept);
    }
    // Only crossings between what stays and what goes count.
    let marks: Vec<Vec2> = marks.into_iter().filter(|m| kept.iter().any(|p| p.a.dist(*m) <= MEET || p.b.dist(*m) <= MEET)).collect();
    if !gone || marks.len() < 2 {
        return Ok(outline.to_vec());
    }
    // The stretches of the cutter's own edge that are inside the outline.
    match cutter {
        Cutter::Disc { center, radius } => {
            let mut at: Vec<(f64, Vec2)> = marks.iter().map(|m| ((*m - *center).angle(), *m)).collect();
            at.sort_by(|x, y| x.0.total_cmp(&y.0));
            for i in 0..at.len() {
                let (from, to) = (at[i], at[(i + 1) % at.len()]);
                let piece = Piece { a: from.1, b: to.1, arc: Some((*center, *radius)), tag };
                if within(&region, piece.mid()) {
                    kept.push(piece);
                }
            }
        }
        Cutter::Polygon(corners) => {
            let n = corners.len();
            for i in 0..n {
                let (p, q) = (corners[i], corners[(i + 1) % n]);
                let e = q - p;
                let length = e.len();
                if length <= MEET {
                    continue;
                }
                // The marks on this side, by how far along it they are.
                let mut stops: Vec<(f64, Vec2)> = vec![(0.0, p), (1.0, q)];
                for m in &marks {
                    let t = (*m - p).dot(e) / (length * length);
                    if (-1e-9..=1.0 + 1e-9).contains(&t) && ((*m - p).cross(e) / length).abs() <= MEET {
                        stops.push((t, *m));
                    }
                }
                stops.sort_by(|x, y| x.0.total_cmp(&y.0));
                for w in stops.windows(2) {
                    let (s, t) = (w[0].1, w[1].1);
                    if s.dist(t) > MEET && within(&region, (s + t) / 2.0) {
                        kept.push(Piece::line(s, t, tag));
                    }
                }
            }
        }
    }
    chained(kept).map_err(|why| match why {
        "apart" => "A corner relief cuts the flange in two: make the relief smaller, or the flange longer.".to_owned(),
        _ => "A corner relief just touches the side of the flange, which leaves an outline that cannot be cut: change the relief's size a little."
            .to_owned(),
    })
}

/// The outline as the kernel takes it.
pub fn to_loop(pieces: &[Piece]) -> Loop {
    Loop { curves: pieces.iter().map(Piece::curve).collect() }
}

/// The area inside an outline (its pieces in order, either way round).
pub fn area(pieces: &[Piece]) -> f64 {
    // Walked from end to end; an arc met backwards counts backwards.
    let mut total = 0.0;
    let mut here = match pieces {
        [first, second, ..] if first.a.dist(second.a) <= MEET || first.a.dist(second.b) <= MEET => first.b,
        [first, ..] => first.a,
        [] => return 0.0,
    };
    for p in pieces {
        let forward = p.a.dist(here) <= MEET;
        let (from, to) = if forward { (p.a, p.b) } else { (p.b, p.a) };
        total += from.cross(to) / 2.0;
        if let Some((c, r)) = p.arc {
            // The segment between the chord and the arc.
            let sweep = p.sweep(c);
            let segment = r * r * (sweep - sweep.sin()) / 2.0;
            total += if forward { segment } else { -segment };
        }
        here = to;
    }
    total.abs()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::f64::consts::PI;

    fn rect(w: f64, h: f64) -> Vec<Piece> {
        polygon(&[(Vec2::new(0.0, 0.0), 1), (Vec2::new(w, 0.0), 2), (Vec2::new(w, h), 3), (Vec2::new(0.0, h), 4)])
    }

    #[test]
    fn a_disc_takes_a_round_bite_out_of_a_corner_or_a_side() {
        // A quarter of a disc of radius 3 out of the corner of a 10 x 6 rectangle.
        let out = cut(&rect(10.0, 6.0), &Cutter::Disc { center: Vec2::ZERO, radius: 3.0 }, 9).unwrap();
        assert!((area(&out) - (60.0 - 9.0 * PI / 4.0)).abs() < 1e-12, "{}", area(&out));
        assert_eq!(out.len(), 5, "{out:?}");
        let bite: Vec<&Piece> = out.iter().filter(|p| p.arc.is_some()).collect();
        assert!(bite.len() == 1 && bite[0].tag == 9 && bite[0].a.near(Vec2::new(3.0, 0.0), 1e-12) && bite[0].b.near(Vec2::new(0.0, 3.0), 1e-12));
        // The sides it shortened keep their tags, and every piece starts where another ends.
        assert_eq!(out.iter().filter(|p| p.tag == 1).count(), 1);
        for p in &out {
            assert_eq!(out.iter().filter(|q| q.a.dist(p.a) < 1e-9 || q.b.dist(p.a) < 1e-9).count(), 2, "{p:?}");
        }
        // Out of the middle of a side: a half disc.
        let out = cut(&rect(10.0, 6.0), &Cutter::Disc { center: Vec2::new(5.0, 0.0), radius: 2.0 }, 9).unwrap();
        assert!((area(&out) - (60.0 - 2.0 * PI)).abs() < 1e-12 && out.len() == 6, "{out:?}");
        // Over a whole end: the end is the arc.
        let out = cut(&rect(4.0, 20.0), &Cutter::Disc { center: Vec2::new(2.0, -1.0), radius: 3.0 }, 9).unwrap();
        let rise = 5f64.sqrt();
        let gone = rise * 2.0 + 9.0 * (2.0f64 / 3.0).asin() - 4.0;
        assert!((area(&out) - (80.0 - gone)).abs() < 1e-12 && out.len() == 4, "{} {out:?}", area(&out));
        // The kernel's arcs run counter-clockwise: so does this one, from the right to the left.
        let Curve2::Arc { start_angle, end_angle, .. } = out.iter().find(|p| p.arc.is_some()).unwrap().curve().curve else { panic!() };
        assert!(start_angle < end_angle && (start_angle - rise.atan2(2.0)).abs() < 1e-12, "{start_angle} {end_angle}");
    }

    #[test]
    fn a_square_takes_a_notch_and_what_misses_or_swallows_the_outline_is_told() {
        let square =
            |x: f64, y: f64, s: f64| Cutter::Polygon(vec![Vec2::new(x, y), Vec2::new(x + s, y), Vec2::new(x + s, y + s), Vec2::new(x, y + s)]);
        // A 4 square notch out of the corner, given either way round.
        for cutter in
            [square(-2.0, -2.0, 4.0), Cutter::Polygon(vec![Vec2::new(-2.0, -2.0), Vec2::new(-2.0, 2.0), Vec2::new(2.0, 2.0), Vec2::new(2.0, -2.0)])]
        {
            let out = cut(&rect(10.0, 6.0), &cutter, 9).unwrap();
            assert!((area(&out) - 56.0).abs() < 1e-12 && out.len() == 6, "{out:?}");
            assert_eq!(out.iter().filter(|p| p.tag == 9).count(), 2);
        }
        // A notch that takes the whole width of an end shortens it: still four sides.
        let out = cut(&rect(4.0, 20.0), &square(-1.0, -3.0, 6.0), 9).unwrap();
        assert!((area(&out) - 68.0).abs() < 1e-12 && out.len() == 4, "{out:?}");
        // A lozenge (a relief at a corner that is not square).
        let lozenge = Cutter::Polygon(vec![Vec2::new(0.0, -2.0), Vec2::new(2.0, 0.0), Vec2::new(0.0, 2.0), Vec2::new(-2.0, 0.0)]);
        let out = cut(&rect(10.0, 6.0), &lozenge, 9).unwrap();
        assert!((area(&out) - 58.0).abs() < 1e-12 && out.len() == 5, "{out:?}");
        // Clear of the outline: nothing changes. Over all of it: nothing is left.
        assert_eq!(cut(&rect(10.0, 6.0), &square(20.0, 0.0, 3.0), 9).unwrap(), rect(10.0, 6.0));
        assert_eq!(cut(&rect(10.0, 6.0), &Cutter::Disc { center: Vec2::new(-5.0, -5.0), radius: 7.0 }, 9).unwrap(), rect(10.0, 6.0));
        assert!(cut(&rect(10.0, 6.0), &Cutter::Disc { center: Vec2::new(5.0, 3.0), radius: 20.0 }, 9).unwrap().is_empty());
        // Right across: two pieces, which is refused.
        let e = cut(&rect(10.0, 2.0), &Cutter::Disc { center: Vec2::new(5.0, 1.0), radius: 3.0 }, 9).unwrap_err();
        assert!(e.contains("in two"), "{e}");
        // A second relief where the first one's arc is: refused too.
        let once = cut(&rect(10.0, 6.0), &Cutter::Disc { center: Vec2::ZERO, radius: 3.0 }, 9).unwrap();
        assert!(cut(&once, &Cutter::Disc { center: Vec2::new(1.0, 0.0), radius: 3.0 }, 8).unwrap_err().contains("run into each other"));
        // One at each end is fine.
        let twice = cut(&once, &Cutter::Disc { center: Vec2::new(10.0, 6.0), radius: 3.0 }, 8).unwrap();
        assert!((area(&twice) - (60.0 - 9.0 * PI / 2.0)).abs() < 1e-12 && twice.len() == 6, "{twice:?}");
    }
}
