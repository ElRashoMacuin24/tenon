//! Robust geometric predicates: Shewchuk's adaptive exact arithmetic, via the `robust` crate.
//!
//! Use these for every orientation or in-circle decision. A naive determinant can return the
//! wrong sign for nearly degenerate input, which breaks topology decisions downstream.

use robust::{Coord, Coord3D};

use crate::{Vec2, Vec3};

fn c2(p: Vec2) -> Coord<f64> {
    Coord { x: p.x, y: p.y }
}

fn c3(p: Vec3) -> Coord3D<f64> {
    Coord3D { x: p.x, y: p.y, z: p.z }
}

/// Twice the signed area of triangle `abc`, with an exact sign: positive when `a, b, c` turn
/// counter-clockwise, negative when clockwise, zero when collinear.
pub fn orient2d(a: Vec2, b: Vec2, c: Vec2) -> f64 {
    robust::orient2d(c2(a), c2(b), c2(c))
}

/// Positive when `d` lies inside the circle through `a, b, c` (given counter-clockwise),
/// negative outside, zero on the circle. The sign is exact.
pub fn incircle(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> f64 {
    robust::incircle(c2(a), c2(b), c2(c), c2(d))
}

/// Positive when `d` lies below the plane through `a, b, c`, where "below" means `a, b, c`
/// appear counter-clockwise when seen from above; negative above, zero when coplanar. The sign
/// is exact.
pub fn orient3d(a: Vec3, b: Vec3, c: Vec3, d: Vec3) -> f64 {
    robust::orient3d(c3(a), c3(b), c3(c), c3(d))
}

/// Turn direction of `a -> b -> c`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    CounterClockwise,
    Clockwise,
    Collinear,
}

/// Exact turn direction of `a -> b -> c`.
pub fn orientation(a: Vec2, b: Vec2, c: Vec2) -> Orientation {
    let d = orient2d(a, b, c);
    if d > 0.0 {
        Orientation::CounterClockwise
    } else if d < 0.0 {
        Orientation::Clockwise
    } else {
        Orientation::Collinear
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orientation_signs() {
        let (a, b) = (Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0));
        assert_eq!(orientation(a, b, Vec2::new(0.5, 1.0)), Orientation::CounterClockwise);
        assert_eq!(orientation(a, b, Vec2::new(0.5, -1.0)), Orientation::Clockwise);
        assert_eq!(orientation(a, b, Vec2::new(7.0, 0.0)), Orientation::Collinear);
    }

    #[test]
    fn exact_where_naive_determinant_rounds_to_zero() {
        // c lies one ulp above the line y = x. The naive determinant loses that ulp when it
        // subtracts 12 and returns 0; the adaptive predicate keeps the exact sign.
        let (a, b) = (Vec2::new(12.0, 12.0), Vec2::new(24.0, 24.0));
        let c = Vec2::new(0.5, 0.5 + f64::EPSILON / 2.0);
        assert!(c.y > 0.5);
        let naive = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
        assert_eq!(naive, 0.0);
        assert_eq!(orientation(a, b, c), Orientation::CounterClockwise);
    }

    #[test]
    fn incircle_signs() {
        let (a, b, c) = (Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0), Vec2::new(-1.0, 0.0));
        assert!(incircle(a, b, c, Vec2::new(0.0, 0.0)) > 0.0);
        assert!(incircle(a, b, c, Vec2::new(2.0, 2.0)) < 0.0);
        assert_eq!(incircle(a, b, c, Vec2::new(0.0, -1.0)), 0.0);
    }

    #[test]
    fn orient3d_signs() {
        let (a, b, c) = (Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
        assert!(orient3d(a, b, c, Vec3::new(0.0, 0.0, -1.0)) > 0.0);
        assert!(orient3d(a, b, c, Vec3::new(0.0, 0.0, 1.0)) < 0.0);
        assert_eq!(orient3d(a, b, c, Vec3::new(5.0, 5.0, 0.0)), 0.0);
    }
}
