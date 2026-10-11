//! The geometry of a flange: where its bend lies, the cross-section of the bend and the wall,
//! and what is cut from the sheet to make room for them.
//!
//! Everything is worked out in the cross-section of the sheet at the edge: `u` runs out of the
//! sheet through the edge (in the sheet's plane), `v` up from the face the edge is on (the side
//! the flange bends towards), and the sheet lies in `u <= 0`, `-t <= v <= 0`. A third
//! coordinate runs along the edge.

use std::f64::consts::FRAC_PI_2;

use tenon_geom::{Frame, Vec2, Vec3};
use tenon_kernel::{Curve2, Loop, Profile, Region, TaggedCurve2};

use super::{BendPosition, CornerShape, FlangeWidth, HeightDatum, ReliefShape, Sizes};

/// An edge of the sheet a flange grows from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Site {
    /// The end of the edge the others are measured from.
    pub origin: Vec3,
    /// Out of the sheet, in its plane, square to the edge.
    pub out: Vec3,
    /// The normal of the face the edge is on: the way the flange bends.
    pub up: Vec3,
    /// Along the edge from `origin` (`out` x `up`).
    pub along: Vec3,
    pub length: f64,
}

impl Site {
    /// The point `u` out, `v` up and `w` along from the origin.
    pub fn at(&self, u: f64, v: f64, w: f64) -> Vec3 {
        self.origin + self.out * u + self.up * v + self.along * w
    }
    /// The frame of the cross-section `w` along the edge: X out, Y up, Z along.
    pub fn section_frame(&self, w: f64) -> Option<Frame> {
        Frame::new(self.at(0.0, 0.0, w), self.along, self.out)
    }
    /// A frame looking down on the sheet from `v` above it: X out, Y along, Z down.
    pub fn plan_frame(&self, v: f64) -> Option<Frame> {
        Frame::new(self.at(0.0, v, 0.0), -self.up, self.out)
    }
}

/// The sizes of one bend and its wall.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    pub t: f64,
    /// The inside radius of the bend.
    pub r: f64,
    pub angle: f64,
    /// How far in from the edge the bend starts (0 when it starts at the edge).
    pub setback: f64,
    /// The length of the flat wall after the bend.
    pub straight: f64,
}

/// How far in from the edge the bend starts.
pub fn setback(position: BendPosition, r: f64, t: f64, angle: f64) -> f64 {
    match position {
        BendPosition::Adjacent => 0.0,
        BendPosition::Inside => (r + t) * (angle / 2.0).tan(),
        BendPosition::Outside => r * (angle / 2.0).tan(),
        BendPosition::Tangent => (r + t) * angle.min(FRAC_PI_2).sin(),
    }
}

/// What the bend takes of a flange's height, measured from `datum`.
pub fn bend_height(datum: HeightDatum, r: f64, t: f64, angle: f64) -> f64 {
    match datum {
        HeightDatum::Outer => (r + t) * (angle / 2.0).tan(),
        HeightDatum::Inner => r * (angle / 2.0).tan(),
        HeightDatum::Tangent => (r + t) * angle.min(FRAC_PI_2).sin(),
    }
}

impl Shape {
    pub fn new(sizes: &Sizes, radius: Option<f64>, angle: f64, height: f64, datum: HeightDatum, position: BendPosition) -> Result<Shape, String> {
        let (t, r) = (sizes.thickness, radius.unwrap_or(sizes.bend_radius));
        let taken = bend_height(datum, r, t, angle);
        let straight = height - taken;
        if straight < -1e-9 {
            return Err(format!(
                "The flange is {} mm high and its bend alone takes {} mm: make it higher, or the bend radius smaller.",
                crate::explain::mm(height),
                crate::explain::mm(taken)
            ));
        }
        Ok(Shape { t, r, angle, setback: setback(position, r, t, angle), straight: straight.max(0.0) })
    }

    /// The centre of the bend in the cross-section.
    fn centre(&self) -> Vec2 {
        Vec2::new(-self.setback, self.r)
    }

    /// Where the wall's inner and outer faces end, in the cross-section.
    pub fn tip(&self) -> (Vec2, Vec2) {
        let (s, c) = self.angle.sin_cos();
        let (round, on) = (Vec2::new(s, -c), Vec2::new(c, s));
        (self.centre() + round * self.r + on * self.straight, self.centre() + round * (self.r + self.t) + on * self.straight)
    }

    /// The cross-section of the bend and the wall: one closed region, its curves tagged with the
    /// parts of [`Part`].
    pub fn section(&self, tag: impl Fn(Part) -> u64) -> Region {
        let c = self.centre();
        let (sin, cos) = self.angle.sin_cos();
        let round = Vec2::new(sin, -cos);
        let (inner_end, outer_end) = (c + round * self.r, c + round * (self.r + self.t));
        let (inner_tip, outer_tip) = self.tip();
        let (root_in, root_out) = (Vec2::new(-self.setback, 0.0), Vec2::new(-self.setback, -self.t));
        let line = |part: Part, start: Vec2, end: Vec2| TaggedCurve2 { tag: tag(part), curve: Curve2::Line { start, end } };
        let arc = |part: Part, radius: f64| TaggedCurve2 {
            tag: tag(part),
            curve: Curve2::Arc { center: c, radius, start_angle: -FRAC_PI_2, end_angle: -FRAC_PI_2 + self.angle },
        };
        let mut curves = vec![arc(Part::BendOuter, self.r + self.t)];
        if self.straight > 1e-9 {
            curves.push(line(Part::Outer, outer_end, outer_tip));
            curves.push(line(Part::Tip, outer_tip, inner_tip));
            curves.push(line(Part::Inner, inner_tip, inner_end));
        } else {
            curves.push(line(Part::Tip, outer_end, inner_end));
        }
        curves.push(arc(Part::BendInner, self.r));
        curves.push(line(Part::Root, root_in, root_out));
        Region { outer: Loop { curves }, holes: vec![] }
    }
}

/// The faces a flange makes, for their names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// The outside of the bend.
    BendOuter = 1,
    /// The wall's outer face.
    Outer = 2,
    /// The wall's far edge.
    Tip = 3,
    /// The wall's inner face.
    Inner = 4,
    /// The inside of the bend.
    BendInner = 5,
    /// Where the bend joins the sheet (inside the part once it is joined).
    Root = 6,
    /// The two ends of the flange.
    Start = 7,
    End = 8,
    /// What the cut for the bend and its reliefs leaves in the sheet.
    Cut = 9,
    /// What a corner relief leaves.
    Corner = 10,
}

/// The stretch of its edge a flange takes: from, to (measured along the edge).
pub fn span(width: &FlangeWidth, length: f64) -> Result<(f64, f64), String> {
    let (a, b) = match *width {
        FlangeWidth::Edge => (0.0, length),
        FlangeWidth::Centered { width } => ((length - width) / 2.0, (length + width) / 2.0),
        FlangeWidth::Offset { start, end } => (start, length - end),
    };
    if a < -1e-9 || b > length + 1e-9 {
        return Err(format!("The flange is wider than its edge, which is {} mm long.", crate::explain::mm(length)));
    }
    if b - a < 1e-6 {
        return Err(format!("The offsets leave nothing of the edge, which is {} mm long.", crate::explain::mm(length)));
    }
    Ok((a.max(0.0), b.min(length)))
}

/// What is cut from the sheet for one flange, seen from above (`u` out of the sheet, `w` along
/// the edge): the strip the bend replaces, `from`..`to` along the edge and `setback` deep, and a
/// relief beside each end that is not at an end of the edge (or at a corner the flange shares
/// with another, where the corner's own relief serves).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Notch {
    pub from: f64,
    pub to: f64,
    pub setback: f64,
    /// A relief before `from`, and one after `to`.
    pub relief: [bool; 2],
    pub shape: ReliefShape,
    pub width: f64,
    /// How far the relief goes in past the bend.
    pub depth: f64,
    /// How far past the edge the cut reaches outwards (so that it leaves no skin).
    pub over: f64,
}

impl Notch {
    /// The reliefs a flange needs: one at each end that stops short of its edge's end, when the
    /// bend is cut into the sheet at all. A strip left between a relief and the end of the edge
    /// that is narrower than `min_remnant` goes too.
    pub fn new(span: (f64, f64), length: f64, shape: &Shape, sizes: &Sizes, relief: ReliefShape, shared: [bool; 2]) -> Notch {
        let cut_in = shape.setback > 1e-9;
        let (mut from, mut to) = span;
        let width = sizes.relief_width;
        let mut wanted = [cut_in && from > 1e-9 && !shared[0], cut_in && length - to > 1e-9 && !shared[1]];
        // At a corner shared with another flange the sheet is cut back to the end of the edge:
        // the corner between the two bends goes.
        if shared[0] {
            from = 0.0;
        }
        if shared[1] {
            to = length;
        }
        // Reliefs that would leave too thin a strip: the cut runs to the end of the edge instead.
        if wanted[0] && from - width < sizes.min_remnant {
            (from, wanted[0]) = (0.0, false);
        }
        if wanted[1] && length - to - width < sizes.min_remnant {
            (to, wanted[1]) = (length, false);
        }
        let depth = match relief {
            ReliefShape::Straight => sizes.relief_depth,
            // A round end is half the width deep at least.
            ReliefShape::Round => sizes.relief_depth.max(width / 2.0),
        };
        Notch { from, to, setback: shape.setback, relief: wanted, shape: relief, width, depth, over: shape.t }
    }

    /// The outline of the cut seen from above, counter-clockwise with `u` to the right and `w`
    /// up; every curve has `tag`.
    pub fn outline(&self, tag: u64) -> Loop {
        let (deep, shallow) = (-(self.setback + self.depth), -self.setback);
        let (low, high) = (self.from - if self.relief[0] { self.width } else { 0.0 }, self.to + if self.relief[1] { self.width } else { 0.0 });
        let r = self.width / 2.0;
        let mut pen = Pen { curves: Vec::new(), at: Vec2::new(self.over, low), tag };
        // In along a relief from `top` and round its end to `bottom`.
        let relief = |pen: &mut Pen, top: f64, bottom: f64| match self.shape {
            ReliefShape::Straight => {
                pen.line(Vec2::new(deep, top));
                pen.line(Vec2::new(deep, bottom));
            }
            ReliefShape::Round => {
                pen.line(Vec2::new(deep + r, top));
                pen.half_round(Vec2::new(deep + r, bottom + r), r);
            }
        };
        // Up the outer side, then back down the inner one: the far relief, the bend's strip,
        // the near relief.
        pen.line(Vec2::new(self.over, high));
        if self.relief[1] {
            relief(&mut pen, high, self.to);
        }
        pen.line(Vec2::new(shallow, self.to));
        pen.line(Vec2::new(shallow, self.from));
        if self.relief[0] {
            relief(&mut pen, self.from, low);
        }
        pen.line(Vec2::new(self.over, low));
        Loop { curves: pen.curves }
    }
}

/// Draws an outline: each curve from where the last one ended.
struct Pen {
    curves: Vec<TaggedCurve2>,
    at: Vec2,
    tag: u64,
}

impl Pen {
    fn line(&mut self, to: Vec2) {
        if self.at.dist(to) > 1e-9 {
            self.curves.push(TaggedCurve2 { tag: self.tag, curve: Curve2::Line { start: self.at, end: to } });
        }
        self.at = to;
    }
    /// Half a circle about `center` from straight above it round the left to straight below.
    fn half_round(&mut self, center: Vec2, radius: f64) {
        self.curves.push(TaggedCurve2 { tag: self.tag, curve: Curve2::Arc { center, radius, start_angle: FRAC_PI_2, end_angle: 3.0 * FRAC_PI_2 } });
        self.at = center - Vec2::new(0.0, radius);
    }
}

/// The outline of a corner relief, seen from above. The two bend lines cross at `at` and run
/// on from there along `d1` and `d2` (unit directions), with the sheet between them:
/// - round: a hole of diameter `size` that touches both bend lines, opened to the corner;
/// - square: a notch `size` along each bend line;
/// - trim: nothing (the bends simply end at the corner).
pub fn corner_outline(shape: CornerShape, at: Vec2, d1: Vec2, d2: Vec2, size: f64, tag: u64) -> Option<Loop> {
    let turn = d1.cross(d2);
    if shape == CornerShape::Trim || size < 1e-6 || turn.abs() < 1e-9 {
        return None;
    }
    let line = |start: Vec2, end: Vec2| TaggedCurve2 { tag, curve: Curve2::Line { start, end } };
    match shape {
        CornerShape::Trim => None,
        CornerShape::Square => {
            let p = [at, at + d1 * size, at + (d1 + d2) * size, at + d2 * size];
            Some(Loop { curves: (0..4).map(|i| line(p[i], p[(i + 1) % 4])).collect() })
        }
        CornerShape::Round => {
            // Half the angle between the bend lines.
            let half = (d1.dot(d2).clamp(-1.0, 1.0).acos()) / 2.0;
            let r = size / 2.0;
            let center = at + (d1 + d2).normalized() * (r / half.sin());
            let (t1, t2) = (at + d1 * (r / half.tan()), at + d2 * (r / half.tan()));
            // Round the far side of the hole: counter-clockwise from the bend line on the right.
            let (from, to) = if turn > 0.0 { (t1, t2) } else { (t2, t1) };
            let arc = TaggedCurve2 {
                tag,
                curve: Curve2::Arc { center, radius: r, start_angle: (from - center).angle(), end_angle: (to - center).angle() },
            };
            Some(Loop { curves: vec![line(at, from), arc, line(to, at)] })
        }
    }
}

/// A profile of one loop in `frame`.
pub fn profile(frame: Frame, outer: Loop) -> Profile {
    Profile { frame, regions: vec![Region { outer, holes: vec![] }] }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::f64::consts::PI;

    fn sizes() -> Sizes {
        Sizes { thickness: 2.0, bend_radius: 3.0, relief_width: 2.0, relief_depth: 1.0, min_remnant: 4.0, corner_size: 8.0, k_factor: 0.44 }
    }

    #[test]
    fn the_bend_is_placed_and_the_height_measured_as_asked() {
        let q = FRAC_PI_2;
        // A quarter turn, radius 3 in 2 mm sheet: the outer faces meet 5 from the bend's start.
        assert!((setback(BendPosition::Inside, 3.0, 2.0, q) - 5.0).abs() < 1e-12);
        assert!((setback(BendPosition::Outside, 3.0, 2.0, q) - 3.0).abs() < 1e-12);
        assert_eq!(setback(BendPosition::Adjacent, 3.0, 2.0, q), 0.0);
        assert!((setback(BendPosition::Tangent, 3.0, 2.0, q) - 5.0).abs() < 1e-12);
        // At 60 degrees the faces meet nearer: (r + t) tan 30; the outside of the bend reaches
        // (r + t) sin 60.
        let a = PI / 3.0;
        assert!((setback(BendPosition::Inside, 3.0, 2.0, a) - 5.0 * (PI / 6.0).tan()).abs() < 1e-12);
        assert!((setback(BendPosition::Tangent, 3.0, 2.0, a) - 5.0 * a.sin()).abs() < 1e-12);
        // 20 high overall: 15 of wall after the bend's 5. From the inner faces, 17.
        let s = Shape::new(&sizes(), None, q, 20.0, HeightDatum::Outer, BendPosition::Inside).unwrap();
        assert!((s.straight - 15.0).abs() < 1e-12 && (s.setback - 5.0).abs() < 1e-12 && s.r == 3.0);
        let s = Shape::new(&sizes(), Some(1.0), q, 20.0, HeightDatum::Inner, BendPosition::Adjacent).unwrap();
        assert!((s.straight - 19.0).abs() < 1e-12 && s.setback == 0.0 && s.r == 1.0);
        // The wall stands upright with its outer face through the edge and its top 20 above the
        // sheet's underside.
        let s = Shape::new(&sizes(), None, q, 20.0, HeightDatum::Outer, BendPosition::Inside).unwrap();
        let (inner, outer) = s.tip();
        assert!(outer.near(Vec2::new(0.0, 18.0), 1e-12) && inner.near(Vec2::new(-2.0, 18.0), 1e-12), "{inner:?} {outer:?}");
        // Too low for its own bend: it says how much the bend takes.
        let e = Shape::new(&sizes(), None, q, 4.0, HeightDatum::Outer, BendPosition::Inside).unwrap_err();
        assert!(e.contains("its bend alone takes 5 mm"), "{e}");
        // Exactly the bend and no wall is allowed.
        assert!(Shape::new(&sizes(), None, q, 5.0, HeightDatum::Outer, BendPosition::Inside).unwrap().straight < 1e-12);
    }

    fn area(l: &Loop) -> f64 {
        l.curves
            .iter()
            .map(|c| match &c.curve {
                Curve2::Line { start, end } => start.cross(*end) / 2.0,
                Curve2::Arc { center, radius, start_angle, end_angle } => {
                    let mut d = end_angle - start_angle;
                    while d <= 0.0 {
                        d += 2.0 * PI;
                    }
                    0.5 * (center.x * radius * (end_angle.sin() - start_angle.sin()) - center.y * radius * (end_angle.cos() - start_angle.cos())
                        + radius * radius * d)
                }
                Curve2::Circle { radius, .. } => PI * radius * radius,
                Curve2::BSpline { .. } => 0.0,
            })
            .sum()
    }

    #[test]
    fn the_cross_section_is_the_bend_and_the_wall() {
        let s = Shape::new(&sizes(), None, FRAC_PI_2, 20.0, HeightDatum::Outer, BendPosition::Inside).unwrap();
        let region = s.section(|p| p as u64);
        assert_eq!(region.outer.curves.iter().map(|c| c.tag).collect::<Vec<_>>(), vec![1, 2, 3, 4, 5, 6]);
        // The bend is a quarter ring between radii 3 and 5; the wall 15 by 2.
        let ends: Vec<(Vec2, Vec2)> = region
            .outer
            .curves
            .iter()
            .filter_map(|c| match &c.curve {
                Curve2::Line { start, end } => Some((*start, *end)),
                _ => None,
            })
            .collect();
        assert!(ends[0].0.near(Vec2::new(0.0, 3.0), 1e-12) && ends[0].1.near(Vec2::new(0.0, 18.0), 1e-12), "the outer face: {:?}", ends[0]);
        assert!(ends[3].0.near(Vec2::new(-5.0, 0.0), 1e-12) && ends[3].1.near(Vec2::new(-5.0, -2.0), 1e-12), "the root: {:?}", ends[3]);
        // With no wall the tip closes the bend itself.
        let bare = Shape { straight: 0.0, ..s }.section(|p| p as u64);
        assert_eq!(bare.outer.curves.iter().map(|c| c.tag).collect::<Vec<_>>(), vec![1, 3, 5, 6]);
    }

    #[test]
    fn a_flange_takes_its_stretch_of_the_edge() {
        assert_eq!(span(&FlangeWidth::Edge, 40.0).unwrap(), (0.0, 40.0));
        assert_eq!(span(&FlangeWidth::Centered { width: 10.0 }, 40.0).unwrap(), (15.0, 25.0));
        assert_eq!(span(&FlangeWidth::Offset { start: 5.0, end: 12.0 }, 40.0).unwrap(), (5.0, 28.0));
        assert!(span(&FlangeWidth::Centered { width: 50.0 }, 40.0).unwrap_err().contains("wider than its edge, which is 40 mm"));
        assert!(span(&FlangeWidth::Offset { start: 25.0, end: 15.0 }, 40.0).unwrap_err().contains("leave nothing"));
    }

    #[test]
    fn reliefs_are_cut_beside_a_flange_narrower_than_its_edge() {
        let s = Shape::new(&sizes(), None, FRAC_PI_2, 20.0, HeightDatum::Outer, BendPosition::Inside).unwrap();
        // The whole edge: the strip the bend replaces and nothing else (5 deep, plus the 2 the
        // cut overshoots outwards).
        let n = Notch::new((0.0, 40.0), 40.0, &s, &sizes(), ReliefShape::Straight, [false, false]);
        assert_eq!(n.relief, [false, false]);
        let l = n.outline(9);
        assert_eq!(l.curves.len(), 4);
        assert!((area(&l) - 40.0 * 7.0).abs() < 1e-9, "{}", area(&l));
        // 10 to 30 of the edge: a slot 2 wide and 1 deeper than the bend at each end.
        let n = Notch::new((10.0, 30.0), 40.0, &s, &sizes(), ReliefShape::Straight, [false, false]);
        assert_eq!(n.relief, [true, true]);
        let l = n.outline(9);
        assert!((area(&l) - (20.0 * 7.0 + 2.0 * 2.0 * 8.0)).abs() < 1e-9, "{}", area(&l));
        // Round ends: each slot loses the corners of its end, a square less a half disc.
        let n = Notch::new((10.0, 30.0), 40.0, &s, &sizes(), ReliefShape::Round, [false, false]);
        let l = n.outline(9);
        assert!((area(&l) - (20.0 * 7.0 + 2.0 * (2.0 * 8.0 - (2.0 - PI / 2.0)))).abs() < 1e-9, "{}", area(&l));
        // 3 from the end of the edge: the strip left beside the relief would be 1 wide, under
        // the 4 the rule keeps, so the cut runs out to the end and there is no relief there.
        let n = Notch::new((3.0, 30.0), 40.0, &s, &sizes(), ReliefShape::Straight, [false, false]);
        assert_eq!((n.from, n.relief), (0.0, [false, true]));
        // A bend that starts at the edge cuts nothing, so it needs no relief either.
        let adjacent = Shape::new(&sizes(), None, FRAC_PI_2, 20.0, HeightDatum::Outer, BendPosition::Adjacent).unwrap();
        assert_eq!(Notch::new((10.0, 30.0), 40.0, &adjacent, &sizes(), ReliefShape::Straight, [false, false]).relief, [false, false]);
        // An end at a corner shared with another flange has the corner's relief instead.
        let n = Notch::new((5.0, 30.0), 40.0, &s, &sizes(), ReliefShape::Straight, [true, false]);
        assert_eq!((n.from, n.relief), (0.0, [false, true]), "and the sheet is cut back to the end of the edge there");
    }

    #[test]
    fn corner_reliefs_are_round_square_or_none() {
        // The corner of a sheet where two bend lines cross at right angles.
        let at = Vec2::new(-5.0, 5.0);
        let (d1, d2) = (Vec2::new(0.0, 1.0), Vec2::new(-1.0, 0.0));
        assert!(corner_outline(CornerShape::Trim, at, d1, d2, 8.0, 10).is_none());
        assert!(corner_outline(CornerShape::Round, at, d1, d2, 0.0, 10).is_none());
        // Square: 8 along each bend line. Round: a hole 8 across touching both lines, 4 from
        // the corner, with the corner opened into it: three quarters of the disc and the
        // square between its centre and the corner.
        assert!((area(&corner_outline(CornerShape::Square, at, d1, d2, 8.0, 10).unwrap()) - 64.0).abs() < 1e-9);
        let round = corner_outline(CornerShape::Round, at, d1, d2, 8.0, 10).unwrap();
        assert!((area(&round) - (16.0 + 12.0 * PI)).abs() < 1e-9, "{}", area(&round));
        let Curve2::Arc { center, radius, .. } = &round.curves[1].curve else { panic!("{round:?}") };
        assert!(center.near(Vec2::new(-9.0, 9.0), 1e-12) && *radius == 4.0, "{center:?}");
        // Given the other way round it is the same hole.
        let other = corner_outline(CornerShape::Round, at, d2, d1, 8.0, 10).unwrap();
        assert!((area(&other) - area(&round)).abs() < 1e-9);
        // Bend lines 60 degrees apart: the hole sits further in, and touches them 4 / tan 30
        // from the corner; the square notch is a rhombus.
        let d3 = Vec2::from_angle(PI / 2.0 + PI / 3.0);
        let sharp = corner_outline(CornerShape::Round, at, d1, d3, 8.0, 10).unwrap();
        let reach = 4.0 / (PI / 6.0).tan();
        assert!((area(&sharp) - (4.0 * reach + 16.0 * PI * 240.0 / 360.0)).abs() < 1e-9, "{}", area(&sharp));
        assert!((area(&corner_outline(CornerShape::Square, at, d1, d3, 8.0, 10).unwrap()) - 64.0 * (PI / 3.0).sin()).abs() < 1e-9);
    }
}
