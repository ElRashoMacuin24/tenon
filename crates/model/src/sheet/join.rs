//! A bend between two faces of the sheet that do not touch: the strip of sheet that joins the
//! edge of one to the edge of the other, and how far each face is cut back to make room.
//!
//! Everything is worked out seen end on, in the cross-section of the first face at its edge (as
//! a flange's is): `u` runs out of the face through the edge, `v` up from the side of the sheet
//! the edge is on, and the first face lies in `u <= 0`, `-t <= v <= 0`. The strip sets off
//! along `+u` from that side of the first face and arrives on the side of the second face that
//! is the same side of the sheet, heading into it.
//!
//! Faces in planes that cross are joined by one bend, tangent to both; each face is trimmed or
//! extended to where the bend begins. Faces in parallel planes take two bends
//! ([`DoubleBend`]): side by side (their edges towards one another) an S, one over the other
//! (their edges the same way) a U.

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};

use serde::{Deserialize, Serialize};
use tenon_geom::Vec2;

use super::MIN_BEND_RADIUS;
use super::strip::Step;

/// How two faces in parallel planes are joined.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DoubleBend {
    /// Two equal bends between the edges as they are: neither face is trimmed or extended.
    #[default]
    #[serde(rename = "fix_edges")]
    FixEdges,
    /// Two bends of 45 degrees with a face between them (less, and tangent to one another,
    /// when the faces are too close for that); one face is trimmed or extended.
    #[serde(rename = "45_degree")]
    FortyFive,
    /// Half a circle, its inside radius half the distance between the faces: for faces that lie
    /// one over the other. One face is trimmed or extended.
    #[serde(rename = "full_radius")]
    FullRadius,
    /// Two bends of 90 degrees with a face between them; one face is trimmed or extended.
    #[serde(rename = "90_degree")]
    Ninety,
}

/// The second face seen in the cross-section of the first: the edge of the side of it that the
/// strip arrives on, and the way into the face from there (a unit vector).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Other {
    pub at: Vec2,
    pub into: Vec2,
}

impl Other {
    /// From the edge that was picked on the second face (`edge`), the way out of the face
    /// through it (`out`) and the way the side of the sheet it is on looks (`up`). The strip
    /// has the sheet on its right all the way, so it arrives on the side of the second face
    /// that has the face's thickness to the right of the way in: the one picked, or the other.
    pub fn of(edge: Vec2, out: Vec2, up: Vec2, t: f64) -> Other {
        let into = -out.normalized();
        let left = into.perp();
        Other { at: if up.dot(left) > 0.0 { edge } else { edge + left * t }, into }
    }
}

/// The strip between two faces, and what it takes of them.
#[derive(Clone, Debug, PartialEq)]
pub struct Joined {
    /// Walked from `(-trims[0], 0)` along `+u`, the sheet reaching `t` to its right.
    pub steps: Vec<Step>,
    /// How far each face is cut back from its edge (nothing when it is extended or kept).
    pub trims: [f64; 2],
}

/// Lengths under this are nothing (mm); directions this close are the same.
const EPS: f64 = 1e-9;

/// The radius of the strip's path round a bend: the path is the inside of a bend to the left
/// and the outside of one to the right.
fn path_radius(left: bool, r: f64, t: f64) -> f64 {
    if left { r } else { r + t }
}

/// A straight run of `gap` when the face is that far short of the bend, or a trim of `-gap`
/// when it reaches past it.
fn reach(gap: f64) -> (f64, f64) {
    if gap >= 0.0 { (gap, 0.0) } else { (0.0, -gap) }
}

/// The strip from the first face to `other`, for sheet `t` thick and bends of inside radius `r`.
/// For faces in parallel planes `double` says how, and `flip` keeps the second face's edge
/// where it is, not the first's (where one of them moves).
pub fn join(other: &Other, t: f64, r: f64, double: DoubleBend, flip: bool) -> Result<Joined, String> {
    if !(r.is_finite() && r >= MIN_BEND_RADIUS) {
        return Err(format!("the bend radius must be at least {MIN_BEND_RADIUS} mm"));
    }
    let (at, into) = (other.at, other.into);
    let across = Vec2::X.cross(into);
    if across.abs() > 1e-6 {
        // One bend, tangent to both faces: it turns the strip from the way out of the first
        // face to the way into the second, about where their two sides would cross.
        let turn = across.atan2(Vec2::X.dot(into));
        let radius = path_radius(turn > 0.0, r, t);
        let tangent = radius * (turn.abs() / 2.0).tan();
        // Where the two sides cross, along the first; where the second face's edge is from
        // there, along the way into it.
        let crossing = at.x - into.x * at.y / into.y;
        let edge = (at - Vec2::new(crossing, 0.0)).dot(into);
        let (run1, trim1) = reach(crossing - tangent);
        let (run2, trim2) = reach(edge - tangent);
        return Ok(Joined { steps: vec![Step::Straight(run1), Step::Bend { radius, angle: turn }, Step::Straight(run2)], trims: [trim1, trim2] });
    }
    // Parallel planes: the second face's side is `apart` above the first's, its edge `along`
    // out from the first's.
    let (apart, along) = (at.y, at.x);
    if apart.abs() < 1e-6 {
        return Err("The two faces lie in one plane, and a bend joins faces that do not: there is nothing to bend between them.".into());
    }
    let up = apart > 0.0;
    let side = apart.signum();
    let rise = apart.abs();
    if into.x > 0.0 {
        // Side by side, edge towards edge: an S. The first bend turns towards the second face
        // and the second one back, so one is the inside of its bend and the other the outside.
        let (first, second) = (path_radius(up, r, t), path_radius(!up, r, t));
        let both = first + second;
        // The two bends through `angle` each with `web` straight between them, and how far
        // along the pair reaches.
        let (angle, web, run) = match double {
            DoubleBend::FullRadius => {
                return Err(
                    "Full Radius joins two faces that lie one over the other, and these lie side by side: choose Fix Edges, 45 Degree or 90 Degree."
                        .into(),
                );
            }
            DoubleBend::Ninety => {
                if rise < both - EPS {
                    return Err(format!(
                        "The faces are {} mm apart, and two 90 degree bends of this radius need {} mm: choose 45 Degree or Fix Edges, or a smaller bend radius.",
                        crate::explain::mm(rise),
                        crate::explain::mm(both)
                    ));
                }
                (FRAC_PI_2, (rise - both).max(0.0), both)
            }
            DoubleBend::FortyFive => {
                let web = (rise - both * (1.0 - FRAC_PI_4.cos())) / FRAC_PI_4.sin();
                if web >= 0.0 {
                    (FRAC_PI_4, web, both * FRAC_PI_4.sin() + web * FRAC_PI_4.cos())
                } else {
                    // Too close for a face between them: the bends turn less and are tangent.
                    let angle = (1.0 - rise / both).acos();
                    (angle, 0.0, both * angle.sin())
                }
            }
            DoubleBend::FixEdges => {
                // Both edges stay: the angle at which the two bends and the face between them
                // reach exactly from one to the other.
                let (a, b) = (along, both - rise);
                let reach = a.hypot(b);
                let angle = if reach >= both { (both / reach).asin() - b.atan2(a) } else { f64::NAN };
                let web = if angle.is_finite() && angle.cos().abs() > 1e-9 { (along - both * angle.sin()) / angle.cos() } else { rise - both };
                if !(angle.is_finite() && angle > EPS && angle < PI - 1e-6 && web >= -1e-7) {
                    return Err(format!(
                        "The edges of the two faces are too close for two bends of this radius between them as they are ({} mm along, {} mm apart): choose 45 Degree or 90 Degree, which move an edge, or a smaller bend radius.",
                        crate::explain::mm(along),
                        crate::explain::mm(rise)
                    ));
                }
                (angle, web.max(0.0), along)
            }
        };
        let pair = [Step::Bend { radius: first, angle: side * angle }, Step::Straight(web), Step::Bend { radius: second, angle: -side * angle }];
        // The edge that stays is where its bend begins; the other face reaches to the pair.
        let ((run1, trim1), (run2, trim2)) = match (double, flip) {
            (DoubleBend::FixEdges, _) => ((0.0, 0.0), (0.0, 0.0)),
            (_, false) => ((0.0, 0.0), reach(along - run)),
            (_, true) => (reach(along - run), (0.0, 0.0)),
        };
        let mut steps = vec![Step::Straight(run1)];
        steps.extend(pair);
        steps.push(Step::Straight(run2));
        Ok(Joined { steps, trims: [trim1, trim2] })
    } else {
        // One over the other, their edges the same way: a U, both bends turning the same way.
        let radius = path_radius(up, r, t);
        let turn = match double {
            DoubleBend::FixEdges | DoubleBend::FortyFive => {
                return Err(
                    "The two faces lie one over the other with their edges the same way: choose Full Radius or 90 Degree to join them.".into()
                );
            }
            DoubleBend::FullRadius => {
                // The inside of the bend is the gap between the faces across.
                let inside = rise / 2.0 - if up { 0.0 } else { t };
                if inside < MIN_BEND_RADIUS {
                    return Err(format!(
                        "The faces are too close for a bend between them: the inside of a full radius bend would be {} mm across.",
                        crate::explain::mm((2.0 * inside).max(0.0))
                    ));
                }
                vec![Step::Bend { radius: rise / 2.0, angle: side * PI }]
            }
            DoubleBend::Ninety => {
                if rise < 2.0 * radius - EPS {
                    return Err(format!(
                        "The faces are {} mm apart, and two 90 degree bends of this radius need {} mm: choose Full Radius, or a smaller bend radius.",
                        crate::explain::mm(rise),
                        crate::explain::mm(2.0 * radius)
                    ));
                }
                vec![
                    Step::Bend { radius, angle: side * FRAC_PI_2 },
                    Step::Straight((rise - 2.0 * radius).max(0.0)),
                    Step::Bend { radius, angle: side * FRAC_PI_2 },
                ]
            }
        };
        // The U ends as far out as it began. The second face lies back from its edge, so it
        // is short of the U when its edge is behind where the U is.
        let ((run1, trim1), (run2, trim2)) = if flip { (reach(along), (0.0, 0.0)) } else { ((0.0, 0.0), reach(-along)) };
        let mut steps = vec![Step::Straight(run1)];
        steps.extend(turn);
        steps.push(Step::Straight(run2));
        Ok(Joined { steps, trims: [trim1, trim2] })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::sheet::strip::{Run, walk};

    const T: f64 = 2.0;
    const R: f64 = 3.0;

    /// The same steps, to the last digits.
    fn same(a: &[Step], b: &[Step]) -> bool {
        a.len() == b.len()
            && a.iter().zip(b).all(|(x, y)| match (x, y) {
                (Step::Straight(p), Step::Straight(q)) => (p - q).abs() < 1e-12,
                (Step::Bend { radius: p, angle: a }, Step::Bend { radius: q, angle: b }) => (p - q).abs() < 1e-12 && (a - b).abs() < 1e-12,
                _ => false,
            })
    }

    /// Where the strip ends and the way it is then going.
    fn end(j: &Joined) -> (Vec2, Vec2) {
        let path = walk(Vec2::new(-j.trims[0], 0.0), Vec2::X, &j.steps);
        let last: &Run = path.last().unwrap();
        (last.end(), last.headings().1)
    }

    /// The strip arrives at the second face's edge, moved back by its trim, going into it.
    fn arrives(j: &Joined, other: &Other) -> bool {
        let (at, heading) = end(j);
        at.near(other.at + other.into * j.trims[1], 1e-9) && heading.near(other.into, 1e-9)
    }

    #[test]
    fn faces_in_planes_that_cross_take_one_bend_tangent_to_both() {
        // A wall standing 10 above the sheet, 4 out from its edge, its inner face towards the
        // sheet (so the side picked, the wall's inner face, is the one the strip arrives on).
        let wall = Other::of(Vec2::new(4.0, 10.0), -Vec2::Y, -Vec2::X, T);
        assert_eq!(wall, Other { at: Vec2::new(4.0, 10.0), into: Vec2::Y });
        let j = join(&wall, T, R, DoubleBend::default(), false).unwrap();
        // A quarter turn to the left, inside radius 3: it begins 3 short of where the faces
        // cross, so the sheet is extended by 1 and the wall by 7.
        assert!(same(&j.steps, &[Step::Straight(1.0), Step::Bend { radius: 3.0, angle: FRAC_PI_2 }, Step::Straight(7.0)]), "{j:?}");
        assert_eq!(j.trims, [0.0, 0.0]);
        assert!(arrives(&j, &wall));
        // Picked by the edge of its outer face instead, it is the same wall.
        let outer = Other::of(Vec2::new(6.0, 10.0), -Vec2::Y, Vec2::X, T);
        assert_eq!(outer, wall);

        // The wall down at the sheet and standing on it 2 in from the edge: both are cut back
        // to where the bend begins, the sheet by 2 + 3 and the wall by 3.
        let close = Other::of(Vec2::new(-2.0, 0.0), -Vec2::Y, -Vec2::X, T);
        let j = join(&close, T, R, DoubleBend::default(), false).unwrap();
        assert_eq!(j.steps[1], Step::Bend { radius: 3.0, angle: FRAC_PI_2 });
        assert!((j.trims[0] - 5.0).abs() < 1e-12 && (j.trims[1] - 3.0).abs() < 1e-12, "{j:?}");
        assert!(arrives(&j, &close));

        // A wall going down from the sheet turns the strip to the right: the path is then the
        // outside of the bend, radius 5.
        let down = Other::of(Vec2::new(8.0, -12.0), Vec2::Y, -Vec2::X, T);
        let j = join(&down, T, R, DoubleBend::default(), false).unwrap();
        assert_eq!(j.steps[1], Step::Bend { radius: 5.0, angle: -FRAC_PI_2 });
        assert!(arrives(&j, &down), "{j:?} {:?}", end(&j));
        // At 60 degrees up from the sheet's plane: a turn of 60 degrees, tangent to both.
        let lean = Vec2::from_angle(PI / 3.0);
        let leaning = Other::of(Vec2::new(10.0, 0.0) + lean * 6.0, -lean, lean.perp(), T);
        let j = join(&leaning, T, R, DoubleBend::default(), false).unwrap();
        assert!(matches!(j.steps[1], Step::Bend { radius, angle } if radius == 3.0 && (angle - PI / 3.0).abs() < 1e-12), "{j:?}");
        assert!(arrives(&j, &leaning));
    }

    #[test]
    fn faces_side_by_side_in_parallel_planes_take_an_s() {
        // A second sheet 12 higher, its edge 20 out, facing the first. The strip is the first
        // one's upper side, so it arrives on the second one's upper side: 12 up.
        let other = Other::of(Vec2::new(20.0, 12.0), -Vec2::X, Vec2::Y, T);
        assert_eq!(other, Other { at: Vec2::new(20.0, 12.0), into: Vec2::X });
        // 90 Degree: up round the inside of one bend (3), 4 of face, back round the outside of
        // the other (5): 8 along. The first edge stays; the second face is extended by 12.
        let j = join(&other, T, R, DoubleBend::Ninety, false).unwrap();
        assert_eq!(
            j.steps,
            [
                Step::Straight(0.0),
                Step::Bend { radius: 3.0, angle: FRAC_PI_2 },
                Step::Straight(4.0),
                Step::Bend { radius: 5.0, angle: -FRAC_PI_2 },
                Step::Straight(12.0)
            ]
        );
        assert!(j.trims == [0.0, 0.0] && arrives(&j, &other));
        // With the second edge the one that stays, the first face is extended by the 12.
        let j = join(&other, T, R, DoubleBend::Ninety, true).unwrap();
        assert!(j.steps[0] == Step::Straight(12.0) && j.steps[4] == Step::Straight(0.0) && arrives(&j, &other), "{j:?}");
        // Edges only 5 apart along: the second face is cut back by 3.
        let near = Other { at: Vec2::new(5.0, 12.0), into: Vec2::X };
        let j = join(&near, T, R, DoubleBend::Ninety, false).unwrap();
        assert!(j.trims == [0.0, 3.0] && arrives(&j, &near), "{j:?}");
        // Faces closer than the two radii cannot take two quarter turns.
        let e = join(&Other { at: Vec2::new(20.0, 6.0), into: Vec2::X }, T, R, DoubleBend::Ninety, false).unwrap_err();
        assert!(e.contains("6 mm apart") && e.contains("need 8 mm"), "{e}");

        // 45 Degree: two eighths of a turn, and between them what is left of the 12 after
        // the 8 (1 - cos 45) the bends rise, on the slant.
        let j = join(&other, T, R, DoubleBend::FortyFive, false).unwrap();
        let web = (12.0 - 8.0 * (1.0 - FRAC_PI_4.cos())) / FRAC_PI_4.sin();
        assert!(matches!(j.steps[1], Step::Bend { radius, angle } if radius == 3.0 && angle == FRAC_PI_4));
        assert!(matches!(j.steps[2], Step::Straight(w) if (w - web).abs() < 1e-12), "{j:?}");
        assert!(arrives(&j, &other));
        // Only 1 apart: the bends turn less than 45 degrees and are tangent to one another.
        let low = Other { at: Vec2::new(20.0, 1.0), into: Vec2::X };
        let j = join(&low, T, R, DoubleBend::FortyFive, false).unwrap();
        assert!(
            matches!(j.steps[1], Step::Bend { angle, .. } if (angle - (1.0f64 - 1.0 / 8.0).acos()).abs() < 1e-12)
                && j.steps[2] == Step::Straight(0.0),
            "{j:?}"
        );
        assert!(arrives(&j, &low));

        // Fix Edges: neither face moves; the bends turn as far as it takes to reach.
        let j = join(&other, T, R, DoubleBend::FixEdges, false).unwrap();
        assert!(j.trims == [0.0, 0.0] && j.steps[0] == Step::Straight(0.0) && j.steps[4] == Step::Straight(0.0) && arrives(&j, &other), "{j:?}");
        let Step::Bend { angle, .. } = j.steps[1] else { panic!() };
        assert!(angle > 0.5 && angle < FRAC_PI_2, "{angle}");
        // The second sheet lower than the first: the same, mirrored, the outside bend first.
        let below = Other::of(Vec2::new(20.0, -12.0 - T), -Vec2::X, -Vec2::Y, T);
        assert_eq!(below.at, Vec2::new(20.0, -12.0));
        let k = join(&below, T, R, DoubleBend::FixEdges, false).unwrap();
        assert!(matches!(k.steps[1], Step::Bend { radius, angle: a } if radius == 5.0 && (a + angle).abs() < 1e-12), "{k:?}");
        assert!(arrives(&k, &below));
        // Edges over one another cannot be reached with these radii as they are.
        let e = join(&Other { at: Vec2::new(0.5, 3.0), into: Vec2::X }, T, R, DoubleBend::FixEdges, false).unwrap_err();
        assert!(e.contains("too close"), "{e}");
        // Full Radius is not for these; nor is anything for faces in one plane.
        assert!(join(&other, T, R, DoubleBend::FullRadius, false).unwrap_err().contains("side by side"));
        assert!(join(&Other { at: Vec2::new(20.0, 0.0), into: Vec2::X }, T, R, DoubleBend::FixEdges, false).unwrap_err().contains("in one plane"));
    }

    #[test]
    fn faces_one_over_the_other_take_a_u() {
        // A second sheet over the first, its underside 10 above the first one's top, its edge 4
        // further out. The strip arrives on its underside.
        let over = Other::of(Vec2::new(4.0, 10.0 + T), Vec2::X, Vec2::Y, T);
        assert_eq!(over, Other { at: Vec2::new(4.0, 10.0), into: -Vec2::X });
        // Full Radius: half a circle of inside radius 5, from the first edge; the second face
        // is cut back by the 4 it reaches past it.
        let j = join(&over, T, R, DoubleBend::FullRadius, false).unwrap();
        assert_eq!(j.steps, [Step::Straight(0.0), Step::Bend { radius: 5.0, angle: PI }, Step::Straight(0.0)]);
        assert!(j.trims == [0.0, 4.0] && arrives(&j, &over), "{j:?}");
        // With the second edge the one that stays, the first face is extended by 4.
        let j = join(&over, T, R, DoubleBend::FullRadius, true).unwrap();
        assert!(j.steps[0] == Step::Straight(4.0) && j.trims == [0.0, 0.0] && arrives(&j, &over), "{j:?}");
        // 90 Degree: two quarter turns of inside radius 3 with 4 of face between them.
        let j = join(&over, T, R, DoubleBend::Ninety, false).unwrap();
        assert_eq!(j.steps[1..4], [Step::Bend { radius: 3.0, angle: FRAC_PI_2 }, Step::Straight(4.0), Step::Bend { radius: 3.0, angle: FRAC_PI_2 }]);
        assert!(arrives(&j, &over));
        // Under the first, the strip is the outside of the U: from the first one's top to the
        // second one's underside is 10 + 2 thicknesses, the inside of the bend 10 across.
        let under = Other::of(Vec2::new(-3.0, -10.0 - T), Vec2::X, Vec2::Y, T);
        assert_eq!(under.at, Vec2::new(-3.0, -10.0 - 2.0 * T));
        let j = join(&under, T, R, DoubleBend::FullRadius, false).unwrap();
        assert_eq!(j.steps, [Step::Straight(0.0), Step::Bend { radius: 7.0, angle: -PI }, Step::Straight(3.0)]);
        assert!(arrives(&j, &under));
        // What does not fit, and what is not for these.
        let tight = Other::of(Vec2::new(0.0, 4.0 + T), Vec2::X, Vec2::Y, T);
        assert!(join(&tight, T, R, DoubleBend::Ninety, false).unwrap_err().contains("need 6 mm"));
        assert!(join(&over, T, R, DoubleBend::FixEdges, false).unwrap_err().contains("one over the other"));
        let touching = Other::of(Vec2::new(0.0, -T), Vec2::X, Vec2::Y, T);
        assert!(join(&touching, T, R, DoubleBend::FullRadius, false).unwrap_err().contains("too close"));
    }
}
