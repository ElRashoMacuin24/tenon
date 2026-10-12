//! A strip of sheet seen end on: straight runs joined by bends, as the closed outline of its
//! cross-section. A hem is such a strip pushed along an edge of the sheet; a contour flange is
//! one pushed out of the sketch its path is drawn in.
//!
//! The strip follows a path and reaches from `lo` to `hi` to the left of it (either may be
//! negative: to the right), so the path is one of its faces, or runs through its middle.

use std::f64::consts::{PI, TAU};

use tenon_geom::Vec2;
use tenon_kernel::{Curve2, Loop, Region, TaggedCurve2};

use super::MIN_BEND_RADIUS;

/// One stretch of a path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Run {
    Line {
        from: Vec2,
        to: Vec2,
    },
    /// Round `center`, from the point at angle `from`, turning through `sweep` (counter-clockwise
    /// when positive).
    Arc {
        center: Vec2,
        radius: f64,
        from: f64,
        sweep: f64,
    },
}

impl Run {
    pub fn start(&self) -> Vec2 {
        match *self {
            Run::Line { from, .. } => from,
            Run::Arc { center, radius, from, .. } => Vec2::polar(center, radius, from),
        }
    }
    pub fn end(&self) -> Vec2 {
        match *self {
            Run::Line { to, .. } => to,
            Run::Arc { center, radius, from, sweep } => Vec2::polar(center, radius, from + sweep),
        }
    }
    /// The direction it sets off in, and the one it arrives in.
    pub fn headings(&self) -> (Vec2, Vec2) {
        match *self {
            Run::Line { from, to } => {
                let d = (to - from).normalized();
                (d, d)
            }
            Run::Arc { from, sweep, .. } => {
                let along = |a: f64| Vec2::from_angle(a).perp() * sweep.signum();
                (along(from), along(from + sweep))
            }
        }
    }
    pub fn length(&self) -> f64 {
        match *self {
            Run::Line { from, to } => from.dist(to),
            Run::Arc { radius, sweep, .. } => radius * sweep.abs(),
        }
    }
    /// The same run `d` to the left of the way it goes. An arc that would be turned inside out
    /// has no such run.
    pub fn offset(&self, d: f64) -> Option<Run> {
        match *self {
            Run::Line { from, to } => {
                let left = (to - from).normalized().perp() * d;
                Some(Run::Line { from: from + left, to: to + left })
            }
            Run::Arc { center, radius, from, sweep } => {
                let r = if sweep > 0.0 { radius - d } else { radius + d };
                (r > 1e-9).then_some(Run::Arc { center, radius: r, from, sweep })
            }
        }
    }
    /// The run the other way.
    pub fn reversed(&self) -> Run {
        match *self {
            Run::Line { from, to } => Run::Line { from: to, to: from },
            Run::Arc { center, radius, from, sweep } => Run::Arc { center, radius, from: from + sweep, sweep: -sweep },
        }
    }
    fn curve(&self, tag: u64) -> TaggedCurve2 {
        let curve = match *self {
            Run::Line { from, to } => Curve2::Line { start: from, end: to },
            Run::Arc { center, radius, from, sweep } => {
                let (start_angle, end_angle) = if sweep > 0.0 { (from, from + sweep) } else { (from + sweep, from) };
                Curve2::Arc { center, radius, start_angle, end_angle }
            }
        };
        TaggedCurve2 { tag, curve }
    }
}

/// One step of a path that is walked: straight on, or round a bend.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step {
    Straight(f64),
    /// `radius` is the path's own; a positive `angle` turns left.
    Bend {
        radius: f64,
        angle: f64,
    },
}

/// The path walked from `start`, setting off along `heading`. Steps of no length are left out.
pub fn walk(start: Vec2, heading: Vec2, steps: &[Step]) -> Vec<Run> {
    let (mut at, mut dir) = (start, heading.normalized());
    let mut out = Vec::new();
    for step in steps {
        match *step {
            Step::Straight(length) if length > 1e-9 => {
                out.push(Run::Line { from: at, to: at + dir * length });
                at += dir * length;
            }
            Step::Bend { radius, angle } if angle.abs() > 1e-9 && radius > 1e-12 => {
                let center = at + dir.perp() * radius * angle.signum();
                let run = Run::Arc { center, radius, from: (at - center).angle(), sweep: angle };
                (at, dir) = (run.end(), run.headings().1);
                out.push(run);
            }
            _ => {}
        }
    }
    out
}

/// The cross-section of a strip that follows `path` and reaches from `lo` to `hi` to its left.
///
/// Each run gives two curves, tagged `tag(run, true)` for the one on its left and
/// `tag(run, false)` for the one on its right; the strip's two ends are tagged `ends`. The runs
/// must follow on from one another without a corner, and no bend may be tighter than the sheet
/// can follow.
pub fn outline(path: &[Run], lo: f64, hi: f64, tag: impl Fn(usize, bool) -> u64, ends: [u64; 2]) -> Result<Region, String> {
    let (Some(first), Some(last)) = (path.first(), path.last()) else { return Err("the profile is empty".into()) };
    if hi - lo < 1e-9 {
        return Err("the sheet has no thickness".into());
    }
    for pair in path.windows(2) {
        if pair[0].end().dist(pair[1].start()) > 1e-7 {
            return Err("The profile's curves do not join end to end.".into());
        }
        if pair[0].headings().1.dot(pair[1].headings().0) < 1.0 - 1e-9 {
            return Err("The profile has a corner where a bend was expected: an arc of it must run on smoothly from the curves beside it.".into());
        }
    }
    let side = |d: f64| -> Result<Vec<Run>, String> {
        path.iter()
            .map(|run| {
                let moved = run.offset(d);
                // The inside of a bend: the smaller of its two radii.
                match (run, moved) {
                    (Run::Arc { .. }, Some(Run::Arc { radius, .. })) if radius < MIN_BEND_RADIUS - 1e-12 => None,
                    (_, moved) => moved,
                }
                .ok_or_else(|| {
                    format!(
                        "A bend of the profile is tighter than the sheet can follow: the inside of it would have a radius under {MIN_BEND_RADIUS} mm. Give the bend a larger radius, or put the sheet on its other side."
                    )
                })
            })
            .collect()
    };
    let (left, right) = (side(hi)?, side(lo)?);
    let mut curves: Vec<TaggedCurve2> = left.iter().enumerate().map(|(i, run)| run.curve(tag(i, true))).collect();
    curves.push(
        Run::Line { from: last.offset(hi).map_or(last.end(), |r| r.end()), to: last.offset(lo).map_or(last.end(), |r| r.end()) }.curve(ends[1]),
    );
    curves.extend(right.iter().enumerate().rev().map(|(i, run)| run.curve(tag(i, false))));
    curves.push(
        Run::Line { from: first.offset(lo).map_or(first.start(), |r| r.start()), to: first.offset(hi).map_or(first.start(), |r| r.start()) }
            .curve(ends[0]),
    );
    Ok(Region { outer: Loop { curves }, holes: vec![] })
}

/// The area of that cross-section: the strip's thickness times the length of its middle.
pub fn area(path: &[Run], lo: f64, hi: f64) -> f64 {
    (hi - lo) * path.iter().filter_map(|run| run.offset((lo + hi) / 2.0)).map(|run| run.length()).sum::<f64>()
}

/// Whether a strip that follows `path` from `lo` to `hi` to its left runs through itself: its
/// outline, taken as short straight pieces, crosses itself somewhere.
pub fn crosses(path: &[Run], lo: f64, hi: f64) -> bool {
    let side = |d: f64| -> Vec<Vec2> {
        let mut points: Vec<Vec2> = Vec::new();
        for run in path.iter().filter_map(|r| r.offset(d)) {
            match run {
                Run::Line { from, .. } => points.push(from),
                Run::Arc { center, radius, from, sweep } => {
                    let n = ((sweep.abs() / 0.1).ceil() as usize).max(2);
                    points.extend((0..n).map(|i| Vec2::polar(center, radius, from + sweep * i as f64 / n as f64)));
                }
            }
        }
        points.extend(path.last().and_then(|r| r.offset(d)).map(|r| r.end()));
        points
    };
    let mut ring = side(hi);
    ring.extend(side(lo).into_iter().rev());
    let n = ring.len();
    let piece = |i: usize| (ring[i], ring[(i + 1) % n]);
    for i in 0..n {
        let (a, b) = piece(i);
        for j in i + 2..n {
            if (j + 1) % n == i {
                continue;
            }
            let (c, d) = piece(j);
            let (r, s) = (b - a, d - c);
            let denom = r.cross(s);
            if denom.abs() < 1e-12 {
                continue;
            }
            let (t, u) = ((c - a).cross(s) / denom, (c - a).cross(r) / denom);
            if t > 1e-9 && t < 1.0 - 1e-9 && u > 1e-9 && u < 1.0 - 1e-9 {
                return true;
            }
        }
    }
    false
}

/// Lines and arcs that follow on from one another, with every corner between two lines rounded
/// so that the inside of the bend has radius `inside` when the strip reaches from `lo` to `hi`
/// to the left of the path. Each run of the result says which of `path`'s it is (a bend made at
/// a corner: the two it joins).
pub fn rounded(path: &[Run], inside: f64, lo: f64, hi: f64) -> Result<Vec<(Run, [usize; 2])>, String> {
    let n = path.len();
    // How much of each line the bends at its two ends take.
    let mut taken = vec![[0.0f64; 2]; n];
    let mut bends: Vec<Option<Run>> = vec![None; n];
    for i in 0..n.saturating_sub(1) {
        let (a, b) = (path[i], path[i + 1]);
        if a.end().dist(b.start()) > 1e-7 {
            return Err("The profile's curves do not join end to end.".into());
        }
        let (d0, d1) = (a.headings().1, b.headings().0);
        let turn = d0.cross(d1).atan2(d0.dot(d1));
        if turn.abs() < 1e-9 {
            continue;
        }
        if !(matches!(a, Run::Line { .. }) && matches!(b, Run::Line { .. })) {
            return Err(
                "The profile has a corner at the end of an arc: an arc must run on smoothly from the curves beside it (make them tangent).".into()
            );
        }
        if turn.abs() > PI - 1e-3 {
            return Err("The profile doubles back on itself at a corner.".into());
        }
        // The path's own radius there: the inside face is the one the corner turns towards.
        let radius = if turn > 0.0 { inside + hi } else { inside - lo };
        let back = radius * (turn.abs() / 2.0).tan();
        taken[i][1] = back;
        taken[i + 1][0] = back;
        let from = a.end() - d0 * back;
        let center = from + d0.perp() * radius * turn.signum();
        bends[i] = Some(Run::Arc { center, radius, from: (from - center).angle(), sweep: turn });
    }
    let mut out = Vec::with_capacity(2 * n);
    for i in 0..n {
        match path[i] {
            Run::Line { from, to } => {
                let length = from.dist(to);
                if taken[i][0] + taken[i][1] > length + 1e-9 {
                    return Err(format!(
                        "A line of the profile is {} mm long and the bends at its ends need {} mm of it: make it longer, or the bend radius smaller.",
                        crate::explain::mm(length),
                        crate::explain::mm(taken[i][0] + taken[i][1])
                    ));
                }
                let d = (to - from).normalized();
                let (from, to) = (from + d * taken[i][0], to - d * taken[i][1]);
                if from.dist(to) > 1e-9 {
                    out.push((Run::Line { from, to }, [i, i]));
                }
            }
            arc @ Run::Arc { sweep, .. } => {
                if sweep.abs() >= TAU - 1e-9 {
                    return Err("An arc of the profile goes all the way round.".into());
                }
                out.push((arc, [i, i]));
            }
        }
        if let Some(bend) = bends[i] {
            out.push((bend, [i, i + 1]));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::f64::consts::FRAC_PI_2;

    /// The area inside a region's outline (Green's theorem; either way round).
    fn inside(region: &Region) -> f64 {
        // Walk the curves end to end: each is given in its own direction.
        let ends = |c: &Curve2| match c {
            Curve2::Line { start, end } => (*start, *end),
            Curve2::Arc { center, radius, start_angle, end_angle } => {
                (Vec2::polar(*center, *radius, *start_angle), Vec2::polar(*center, *radius, *end_angle))
            }
            other => panic!("neither a line nor an arc: {other:?}"),
        };
        let curves = &region.outer.curves;
        let mut here = {
            let (a, b) = ends(&curves[0].curve);
            let (c, d) = ends(&curves[1].curve);
            if b.dist(c) < 1e-9 || b.dist(d) < 1e-9 { a } else { b }
        };
        let mut total = 0.0;
        for c in curves {
            let (a, b) = ends(&c.curve);
            let forward = a.dist(here) < 1e-9;
            assert!(forward || b.dist(here) < 1e-9, "the outline breaks at {here:?}");
            let (from, to) = if forward { (a, b) } else { (b, a) };
            total += from.cross(to) / 2.0;
            if let Curve2::Arc { radius, start_angle, end_angle, .. } = &c.curve {
                let sweep = end_angle - start_angle;
                let segment = radius * radius * (sweep - sweep.sin()) / 2.0;
                total += if forward { segment } else { -segment };
            }
            here = to;
        }
        total.abs()
    }

    #[test]
    fn a_walked_path_is_thickened_into_a_cross_section() {
        // Along +X for 10, a quarter turn left at radius 3, then 5 on: a path that ends at
        // (13, 8) heading +Y.
        let path = walk(
            Vec2::ZERO,
            Vec2::X,
            &[Step::Straight(10.0), Step::Bend { radius: 3.0, angle: FRAC_PI_2 }, Step::Straight(5.0), Step::Straight(0.0)],
        );
        assert_eq!(path.len(), 3);
        assert!(path[2].end().near(Vec2::new(13.0, 8.0), 1e-12) && path[2].headings().1.near(Vec2::Y, 1e-12), "{path:?}");
        // The sheet 2 thick on the path's right (the outside of the bend): radii 3 and 5.
        let region = outline(&path, -2.0, 0.0, |i, left| 10 * i as u64 + u64::from(left), [100, 200]).unwrap();
        let want = 2.0 * (10.0 + 5.0) + FRAC_PI_2 / 2.0 * (25.0 - 9.0);
        assert!((inside(&region) - want).abs() < 1e-12 && (area(&path, -2.0, 0.0) - want).abs() < 1e-12, "{}", inside(&region));
        assert_eq!(region.outer.curves.iter().map(|c| c.tag).collect::<Vec<_>>(), vec![1, 11, 21, 200, 20, 10, 0, 100]);
        let radii: Vec<f64> =
            region.outer.curves.iter().filter_map(|c| if let Curve2::Arc { radius, .. } = c.curve { Some(radius) } else { None }).collect();
        assert_eq!(radii, vec![3.0, 5.0]);
        // On its left (the inside of the bend) the bend is too tight: 3 less 2 is fine, 3 less
        // 3 is not.
        assert!(outline(&path, 0.0, 2.0, |_, _| 0, [0, 0]).is_ok());
        let e = outline(&path, 0.0, 3.0, |_, _| 0, [0, 0]).unwrap_err();
        assert!(e.contains("tighter than the sheet can follow"), "{e}");
        // A turn to the right is the mirror of it.
        let right = walk(Vec2::ZERO, Vec2::X, &[Step::Straight(10.0), Step::Bend { radius: 3.0, angle: -FRAC_PI_2 }, Step::Straight(5.0)]);
        assert!(right[2].end().near(Vec2::new(13.0, -8.0), 1e-12));
        assert!((inside(&outline(&right, 0.0, 2.0, |_, _| 0, [0, 0]).unwrap()) - want).abs() < 1e-12);
        // A strip that comes back through itself is found out; one that only comes back beside
        // itself (a hem) is not.
        let hook = walk(
            Vec2::ZERO,
            Vec2::X,
            &[
                Step::Straight(20.0),
                Step::Bend { radius: 3.0, angle: PI },
                Step::Straight(8.0),
                Step::Bend { radius: 1.0, angle: FRAC_PI_2 },
                Step::Straight(12.0),
            ],
        );
        assert!(crosses(&hook, -2.0, 0.0) && !crosses(&hook[..3], -2.0, 0.0) && !crosses(&path, -2.0, 0.0));
        // A path with a corner in it is refused: a strip has bends, not creases.
        let bent = [Run::Line { from: Vec2::ZERO, to: Vec2::new(10.0, 0.0) }, Run::Line { from: Vec2::new(10.0, 0.0), to: Vec2::new(10.0, 5.0) }];
        assert!(outline(&bent, 0.0, 2.0, |_, _| 0, [0, 0]).unwrap_err().contains("corner where a bend was expected"));
    }

    #[test]
    fn the_corners_of_a_drawn_profile_are_rounded_to_the_bend_radius() {
        // An L: 20 along X, then 15 up. The sheet 2 thick on the left of the path (the inside
        // of this corner), bends of inside radius 3: the path itself is the outside, radius 5,
        // and each line gives up 5 to the bend.
        let l = [Run::Line { from: Vec2::ZERO, to: Vec2::new(20.0, 0.0) }, Run::Line { from: Vec2::new(20.0, 0.0), to: Vec2::new(20.0, 15.0) }];
        let path = rounded(&l, 3.0, 0.0, 2.0).unwrap();
        assert_eq!(path.iter().map(|p| p.1).collect::<Vec<_>>(), vec![[0, 0], [0, 1], [1, 1]]);
        let Run::Arc { center, radius, sweep, .. } = path[1].0 else { panic!() };
        assert!(center.near(Vec2::new(15.0, 5.0), 1e-12) && radius == 5.0 && (sweep - FRAC_PI_2).abs() < 1e-12);
        assert!(path[0].0.end().near(Vec2::new(15.0, 0.0), 1e-12) && path[2].0.start().near(Vec2::new(20.0, 5.0), 1e-12));
        let runs: Vec<Run> = path.iter().map(|p| p.0).collect();
        let want = 2.0 * (15.0 + 10.0) + FRAC_PI_2 / 2.0 * (25.0 - 9.0);
        assert!((inside(&outline(&runs, 0.0, 2.0, |_, _| 0, [0, 0]).unwrap()) - want).abs() < 1e-12);
        // The sheet on the other side: the path is the inside of the bend, radius 3.
        let Run::Arc { radius, .. } = rounded(&l, 3.0, -2.0, 0.0).unwrap()[1].0 else { panic!() };
        assert_eq!(radius, 3.0);
        // About its middle: 4.
        let Run::Arc { radius, .. } = rounded(&l, 3.0, -1.0, 1.0).unwrap()[1].0 else { panic!() };
        assert_eq!(radius, 4.0);
        // A Z turns one way and then the other: each bend has its inside at radius 3.
        let z = [
            Run::Line { from: Vec2::ZERO, to: Vec2::new(20.0, 0.0) },
            Run::Line { from: Vec2::new(20.0, 0.0), to: Vec2::new(20.0, 15.0) },
            Run::Line { from: Vec2::new(20.0, 15.0), to: Vec2::new(40.0, 15.0) },
        ];
        let path = rounded(&z, 3.0, 0.0, 2.0).unwrap();
        let radii: Vec<f64> = path.iter().filter_map(|p| if let Run::Arc { radius, .. } = p.0 { Some(radius) } else { None }).collect();
        assert_eq!(radii, vec![5.0, 3.0]);
        // Lines in one direction need no bend; a line too short for its bends says so; an arc
        // that meets a line at a corner is refused.
        let straight = [Run::Line { from: Vec2::ZERO, to: Vec2::new(5.0, 0.0) }, Run::Line { from: Vec2::new(5.0, 0.0), to: Vec2::new(9.0, 0.0) }];
        assert_eq!(rounded(&straight, 3.0, 0.0, 2.0).unwrap().len(), 2);
        let short = [l[0], Run::Line { from: Vec2::new(20.0, 0.0), to: Vec2::new(20.0, 4.0) }];
        let e = rounded(&short, 3.0, 0.0, 2.0).unwrap_err();
        assert!(e.contains("4 mm long and the bends at its ends need 5 mm"), "{e}");
        let arc = Run::Arc { center: Vec2::new(25.0, 0.0), radius: 5.0, from: PI, sweep: -FRAC_PI_2 };
        assert!(rounded(&[l[0], arc], 3.0, 0.0, 2.0).unwrap_err().contains("corner at the end of an arc"));
        // An arc drawn tangent is taken as the bend it is.
        let tangent = Run::Arc { center: Vec2::new(20.0, 5.0), radius: 5.0, from: -FRAC_PI_2, sweep: FRAC_PI_2 };
        assert_eq!(rounded(&[l[0], tangent], 3.0, 0.0, 2.0).unwrap().len(), 2);
    }
}
