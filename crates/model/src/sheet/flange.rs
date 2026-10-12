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

use super::outline::{self, Cutter, Piece};
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
    /// The frame a bend is rolled in: its origin on the bend's axis where the edge begins, Z
    /// along the edge, X down to where the bend starts on the sheet.
    pub fn bend_frame(&self, shape: &Shape) -> Option<Frame> {
        Frame::new(self.at(-shape.setback, shape.r, 0.0), self.along, -self.up)
    }
    /// The frame of a wall's inner face: its origin where the bend ends (and the edge begins), X
    /// up the wall, Y along the edge, Z through the wall to its outer face.
    pub fn wall_frame(&self, shape: &Shape) -> Option<Frame> {
        let (sin, cos) = shape.angle.sin_cos();
        let origin = self.at(-shape.setback + shape.r * sin, shape.r - shape.r * cos, 0.0);
        Frame::new(origin, self.out * sin - self.up * cos, self.out * cos + self.up * sin)
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
    /// What a corner relief leaves in the sheet.
    Corner = 10,
    /// The two ends of the bend, where it is a piece of its own (a flange that meets another).
    BendStart = 11,
    BendEnd = 12,
    /// The underside of a wall that reaches past its bend.
    Foot = 13,
    /// The far side of a bend that reaches past its wall.
    Neck = 14,
    /// What a corner relief leaves in the bend, at the start of the edge and at its end.
    BendReliefStart = 15,
    BendReliefEnd = 16,
    /// What a corner relief leaves in the wall.
    WallReliefStart = 17,
    WallReliefEnd = 18,
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

/// What the rule and the feature say of the corners where two flanges meet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Corners {
    pub shape: CornerShape,
    /// The relief's size: the diameter of a round one, the side of a square one.
    pub size: f64,
    /// The gap left between the two walls. Without one they are not mitred: each wall ends
    /// where its bend does.
    pub gap: Option<f64>,
}

impl Shape {
    /// The width of the bend in the flat, with its neutral surface at radius `rho`.
    pub fn allowance(&self, rho: f64) -> f64 {
        self.angle * rho
    }

    /// How far along its edge from the corner the bend starts, where this flange meets another
    /// across a corner of angle `corner` (between the two edges): where their bend lines cross.
    pub fn back(&self, corner: f64) -> f64 {
        self.setback / (corner / 2.0).tan()
    }

    /// How far along its edge from the corner the wall ends, `h` up the wall from its bend.
    ///
    /// Mitred, the two walls stop either side of the plane that halves the corner, their inner
    /// faces `gap` apart all the way up; each end is cut square through the sheet. Walls that
    /// lean in draw back as they rise, walls that lean out reach on past the corner.
    pub fn wall_end(&self, corner: f64, gap: Option<f64>, h: f64) -> f64 {
        let Some(gap) = gap else { return self.back(corner) };
        let (sin, cos) = self.angle.sin_cos();
        // How far out of the sheet the wall's inner face is there.
        let inner = -self.setback + self.r * sin + h * cos;
        -inner / (corner / 2.0).tan() + gap / 2.0 / (corner / 2.0).sin()
    }

    /// What the corner relief takes out of the flat sheet, in this flange's own flat: `x` out
    /// from where its bend starts, `y` along its edge from the corner. It is centred where the
    /// centre lines of the two bends cross:
    /// - round: a disc `size` across;
    /// - square: a square `size` along each bend line (a lozenge where the corner is not square);
    /// - trim: nothing more than the bends ending at the corner.
    pub fn relief(&self, rho: f64, corner: f64, rule: &Corners) -> Option<Cutter> {
        if rule.size < 1e-6 {
            return None;
        }
        let half = self.allowance(rho) / 2.0;
        let centre = Vec2::new(half, self.back(corner) - half / (corner / 2.0).tan());
        match rule.shape {
            CornerShape::Trim => None,
            CornerShape::Round => Some(Cutter::Disc { center: centre, radius: rule.size / 2.0 }),
            CornerShape::Square => {
                // Half a side along this flange's bend line, and along the other's.
                let (own, other) = (Vec2::new(0.0, rule.size / 2.0), Vec2::new(-corner.sin(), corner.cos()) * (rule.size / 2.0));
                Some(Cutter::Polygon(vec![centre - own - other, centre + own - other, centre + own + other, centre - own + other]))
            }
        }
    }
}

/// A cutter placed by its distance from a corner, as it lies along the edge itself (`y` from the
/// edge's beginning): as it is at the beginning of the edge, turned round at its end.
pub fn at_end(cutter: &Cutter, end: usize, length: f64) -> Cutter {
    let place = |p: Vec2| if end == 0 { p } else { Vec2::new(p.x, length - p.y) };
    match cutter {
        Cutter::Disc { center, radius } => Cutter::Disc { center: place(*center), radius: *radius },
        Cutter::Polygon(corners) => Cutter::Polygon(corners.iter().map(|c| place(*c)).collect()),
    }
}

/// A flange that meets others at corners, as outlines in the flat (`x` out from where the bend
/// starts, `y` along the edge from its beginning): its bend, and its wall when it has one.
#[derive(Clone, Debug, PartialEq)]
pub struct Pieces {
    pub bend: Vec<Piece>,
    pub wall: Vec<Piece>,
}

/// The outlines of the bend and the wall of a flange whose bend takes `span` of an edge
/// `length` long, and meets another flange at the ends that have a corner angle in `meets`.
pub fn pieces(
    shape: &Shape,
    rho: f64,
    span: (f64, f64),
    length: f64,
    meets: [Option<f64>; 2],
    rule: &Corners,
    tag: impl Fn(Part) -> u64,
) -> Result<Pieces, String> {
    let width = shape.allowance(rho);
    let reliefs = [0, 1].map(|e| meets[e].and_then(|corner| shape.relief(rho, corner, rule)).map(|k| at_end(&k, e, length)));
    let relieved = |mut outline: Vec<Piece>, parts: [Part; 2]| -> Result<Vec<Piece>, String> {
        for (relief, part) in reliefs.iter().zip(parts) {
            if let Some(cutter) = relief {
                outline = outline::cut(&outline, cutter, tag(part))?;
            }
        }
        if outline.is_empty() {
            return Err(
                "The corner relief is larger than the flange beside it: make the relief smaller (Corner Relief Size in Sheet Metal Defaults).".into(),
            );
        }
        Ok(outline)
    };
    let bend = relieved(
        outline::polygon(&[
            (Vec2::new(0.0, span.0), tag(Part::BendStart)),
            (Vec2::new(width, span.0), tag(Part::Neck)),
            (Vec2::new(width, span.1), tag(Part::BendEnd)),
            (Vec2::new(0.0, span.1), tag(Part::Root)),
        ]),
        [Part::BendReliefStart, Part::BendReliefEnd],
    )?;
    let high = shape.straight;
    if high < 1e-9 {
        return Ok(Pieces { bend, wall: Vec::new() });
    }
    let low_end = |h: f64| meets[0].map_or(span.0, |corner| shape.wall_end(corner, rule.gap, h));
    let high_end = |h: f64| meets[1].map_or(span.1, |corner| length - shape.wall_end(corner, rule.gap, h));
    if high_end(0.0) - low_end(0.0) < 1e-6 || high_end(high) - low_end(high) < 1e-6 {
        return Err("The flanges on the edges beside it leave nothing of one of the walls: make the flange lower, or leave that edge out.".into());
    }
    let wall = relieved(
        outline::polygon(&[
            (Vec2::new(width, low_end(0.0)), tag(Part::Start)),
            (Vec2::new(width + high, low_end(high)), tag(Part::Tip)),
            (Vec2::new(width + high, high_end(high)), tag(Part::End)),
            (Vec2::new(width, high_end(0.0)), tag(Part::Foot)),
        ]),
        [Part::WallReliefStart, Part::WallReliefEnd],
    )?;
    Ok(Pieces { bend, wall })
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
        Sizes { thickness: 2.0, bend_radius: 3.0, relief_width: 2.0, relief_depth: 1.0, min_remnant: 4.0, corner_size: 8.0, k_factor: 0.44, gap: 2.0 }
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
    fn where_two_flanges_meet_their_bends_end_and_their_walls_are_mitred() {
        let q = FRAC_PI_2;
        let s = Shape::new(&sizes(), None, q, 20.0, HeightDatum::Outer, BendPosition::Inside).unwrap();
        // The bend lines are 5 in from each edge: at a square corner they cross 5 along each,
        // at a blunt one (120 degrees) nearer the corner, at a sharp one further from it.
        assert!((s.back(q) - 5.0).abs() < 1e-12);
        assert!((s.back(2.0 * PI / 3.0) - 5.0 / 3f64.sqrt()).abs() < 1e-12 && (s.back(PI / 3.0) - 5.0 * 3f64.sqrt()).abs() < 1e-12);
        // Upright walls at a square corner: each inner face is 2 in from its edge, so the two
        // meet 2 along; a gap of 2 between them is root 2 more for each, all the way up.
        let end = 2.0 + 2f64.sqrt();
        assert!((s.wall_end(q, Some(2.0), 0.0) - end).abs() < 1e-12 && (s.wall_end(q, Some(2.0), 15.0) - end).abs() < 1e-12);
        // Not mitred, a wall ends where its bend does.
        assert!((s.wall_end(q, None, 7.0) - 5.0).abs() < 1e-12);
        // Walls that lean out 30 degrees (the sheet turns 60) reach on towards the corner as
        // they rise: half as far as they are high. Walls that lean in draw back as much.
        let out = Shape::new(&sizes(), None, PI / 3.0, 20.0, HeightDatum::Outer, BendPosition::Inside).unwrap();
        assert!((out.wall_end(q, Some(2.0), 0.0) - out.wall_end(q, Some(2.0), 10.0) - 5.0).abs() < 1e-12);
        let lean_in = Shape::new(&sizes(), None, 2.0 * PI / 3.0, 20.0, HeightDatum::Outer, BendPosition::Inside).unwrap();
        assert!((lean_in.wall_end(q, Some(2.0), 10.0) - lean_in.wall_end(q, Some(2.0), 0.0) - 5.0).abs() < 1e-12);
        // At a blunt corner the walls meet sooner: 2 / tan 60 along, and the gap takes 1 / sin 60.
        assert!((s.wall_end(2.0 * PI / 3.0, Some(2.0), 3.0) - (2.0 / 3f64.sqrt() + 1.0 / (PI / 3.0).sin())).abs() < 1e-12);
    }

    #[test]
    fn corner_reliefs_are_centred_where_the_bend_centre_lines_cross() {
        let q = FRAC_PI_2;
        let s = Shape::new(&sizes(), None, q, 20.0, HeightDatum::Outer, BendPosition::Inside).unwrap();
        // The neutral surface at 0.44 of the thickness: the bend is a quarter of 2 pi 3.88 wide.
        let rho = 3.0 + 0.44 * 2.0;
        let width = s.allowance(rho);
        assert!((width - FRAC_PI_2 * 3.88).abs() < 1e-12);
        let rule = |shape: CornerShape, size: f64| Corners { shape, size, gap: Some(2.0) };
        assert_eq!(s.relief(rho, q, &rule(CornerShape::Trim, 8.0)), None);
        assert_eq!(s.relief(rho, q, &rule(CornerShape::Round, 0.0)), None);
        // Half the bend's width out from where it starts, and as far short of where the bends
        // begin (5 along the edge).
        let centre = Vec2::new(width / 2.0, 5.0 - width / 2.0);
        let Some(Cutter::Disc { center, radius }) = s.relief(rho, q, &rule(CornerShape::Round, 8.0)) else { panic!() };
        assert!(center.near(centre, 1e-12) && radius == 4.0, "{center:?}");
        let Some(Cutter::Polygon(square)) = s.relief(rho, q, &rule(CornerShape::Square, 8.0)) else { panic!() };
        for (corner, (dx, dy)) in square.iter().zip([(1.0, -1.0), (1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0)]) {
            assert!(corner.near(centre + Vec2::new(4.0 * dx, 4.0 * dy), 1e-12), "{square:?}");
        }
        // At a blunt corner the centre lines cross nearer the corner, and the square is a
        // lozenge with sides along both bend lines.
        let blunt = 2.0 * PI / 3.0;
        let Some(Cutter::Disc { center, .. }) = s.relief(rho, blunt, &rule(CornerShape::Round, 8.0)) else { panic!() };
        assert!(center.near(Vec2::new(width / 2.0, (5.0 - width / 2.0) / 3f64.sqrt()), 1e-12), "{center:?}");
        let Some(Cutter::Polygon(lozenge)) = s.relief(rho, blunt, &rule(CornerShape::Square, 8.0)) else { panic!() };
        let side = |i: usize| lozenge[(i + 1) % 4] - lozenge[i];
        assert!((side(0).len() - 8.0).abs() < 1e-12 && side(0).x.abs() < 1e-12, "along this flange's own bend line: {lozenge:?}");
        assert!((side(1).len() - 8.0).abs() < 1e-12 && (side(1).normalized().dot(Vec2::Y) - blunt.cos()).abs() < 1e-12, "and the other's");
        // The same relief at the far end of a 40 mm edge, turned round.
        let Cutter::Disc { center, .. } = at_end(&Cutter::Disc { center: centre, radius: 4.0 }, 1, 40.0) else { panic!() };
        assert!(center.near(Vec2::new(width / 2.0, 35.0 + width / 2.0), 1e-12));
    }

    #[test]
    fn a_flange_that_meets_another_is_a_bend_and_a_wall_with_the_relief_out_of_both() {
        let q = FRAC_PI_2;
        let s = Shape::new(&sizes(), None, q, 20.0, HeightDatum::Outer, BendPosition::Inside).unwrap();
        let rho = 3.88;
        let width = s.allowance(rho);
        let tag = |p: Part| p as u64;
        // Trimmed to the bends and not mitred: two rectangles, the bend from 5 along the edge
        // (where it meets the other flange's) to its end at 40, and the wall the same.
        let plain = Corners { shape: CornerShape::Trim, size: 8.0, gap: None };
        let p = pieces(&s, rho, (5.0, 40.0), 40.0, [Some(q), None], &plain, tag).unwrap();
        assert!((outline::area(&p.bend) - width * 35.0).abs() < 1e-9 && p.bend.len() == 4);
        assert!((outline::area(&p.wall) - 15.0 * 35.0).abs() < 1e-9 && p.wall.len() == 4);
        assert_eq!(p.bend.iter().map(|k| k.tag).collect::<std::collections::BTreeSet<_>>().len(), 4, "every side its own name");
        // Mitred with a gap of 2: the wall reaches on past the end of its bend, to 2 + root 2
        // from the corner.
        let mitred = Corners { gap: Some(2.0), ..plain };
        let p = pieces(&s, rho, (5.0, 40.0), 40.0, [Some(q), None], &mitred, tag).unwrap();
        assert!((outline::area(&p.wall) - 15.0 * (38.0 - 2f64.sqrt())).abs() < 1e-9);
        // A round relief 8 across is centred 3.05 out and 1.95 along: it bites the middle of
        // the bend's end (a segment of the disc past the chord 3.05 from its centre) and the
        // foot of the wall where it reaches past the bend.
        let round = Corners { shape: CornerShape::Round, ..mitred };
        let p = pieces(&s, rho, (5.0, 40.0), 40.0, [Some(q), None], &round, tag).unwrap();
        let d = width / 2.0;
        let segment = 16.0 * (d / 4.0).acos() - d * (16.0 - d * d).sqrt();
        assert!((outline::area(&p.bend) - (width * 35.0 - segment)).abs() < 1e-9, "{}", outline::area(&p.bend));
        assert_eq!(p.bend.iter().filter(|k| k.arc.is_some() && k.tag == Part::BendReliefStart as u64).count(), 1);
        // The wall's share: between the wall's end (2 + root 2 along) and where the disc leaves
        // the wall's foot.
        let cy = 5.0 - d;
        let under = |y: f64| {
            let s = y - cy;
            (s * (16.0 - s * s).sqrt() + 16.0 * (s / 4.0).asin()) / 2.0 - d * s
        };
        let (from, to) = (2.0 + 2f64.sqrt(), cy + (16.0 - d * d).sqrt());
        let bite = under(to) - under(from);
        assert!(bite > 0.1, "{bite}");
        assert!((outline::area(&p.wall) - (15.0 * (38.0 - 2f64.sqrt()) - bite)).abs() < 1e-9, "{}", outline::area(&p.wall));
        assert_eq!(p.wall.iter().filter(|k| k.arc.is_some() && k.tag == Part::WallReliefStart as u64).count(), 1);
        // A corner at each end: a relief out of each, named apart.
        let p = pieces(&s, rho, (5.0, 35.0), 40.0, [Some(q), Some(q)], &round, tag).unwrap();
        assert!((outline::area(&p.bend) - (width * 30.0 - 2.0 * segment)).abs() < 1e-9);
        assert_eq!(p.bend.iter().filter(|k| k.tag == Part::BendReliefEnd as u64).count(), 1);
        // No wall when the flange is only its bend.
        let bare = Shape::new(&sizes(), None, q, 5.0, HeightDatum::Outer, BendPosition::Inside).unwrap();
        assert!(pieces(&bare, rho, (5.0, 40.0), 40.0, [Some(q), None], &round, tag).unwrap().wall.is_empty());
        // What cannot be: walls mitred at both ends of an edge too short for them, and a
        // relief larger than the flange.
        let e = pieces(&s, rho, (3.0, 3.5), 6.5, [Some(q), Some(q)], &mitred, tag).unwrap_err();
        assert!(e.contains("leave nothing of one of the walls"), "{e}");
        let huge = Corners { size: 400.0, ..round };
        let e = pieces(&s, rho, (5.0, 40.0), 40.0, [Some(q), None], &huge, tag).unwrap_err();
        assert!(e.contains("larger than the flange"), "{e}");
    }
}
