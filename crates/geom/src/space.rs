//! 3D placement types: axes, right-handed frames, axis-aligned boxes.

use serde::{Deserialize, Serialize};

use crate::{Vec2, Vec3, tol};

/// An oriented line: a point and a unit direction. Used for rotation axes and cylinder axes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Axis {
    origin: Vec3,
    dir: Vec3,
}

impl Axis {
    /// The world Z axis through the origin.
    pub const Z: Axis = Axis { origin: Vec3::ZERO, dir: Vec3::Z };

    /// Normalises `dir`. `None` for a zero-length or non-finite direction or a non-finite origin.
    pub fn new(origin: Vec3, dir: Vec3) -> Option<Axis> {
        if !origin.is_finite() || !dir.is_finite() || dir.len() <= tol::LINEAR {
            return None;
        }
        Some(Axis { origin, dir: dir.normalized() })
    }
    pub fn origin(&self) -> Vec3 {
        self.origin
    }
    /// Unit direction.
    pub fn dir(&self) -> Vec3 {
        self.dir
    }
}

/// A right-handed orthonormal coordinate frame: an origin and unit X, Y, Z directions.
///
/// The fields are private so the frame is always orthonormal; construct it with [`Frame::new`]
/// or [`Frame::from_normal`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    origin: Vec3,
    x: Vec3,
    y: Vec3,
    z: Vec3,
}

impl Frame {
    /// The world frame.
    pub const WORLD: Frame = Frame { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Y, z: Vec3::Z };

    /// Frame at `origin` with normal `z`; X is `x_hint` made perpendicular to Z (Gram-Schmidt).
    /// `None` when the inputs are non-finite, `z` is zero, or `x_hint` is parallel to `z`.
    pub fn new(origin: Vec3, z: Vec3, x_hint: Vec3) -> Option<Frame> {
        if !origin.is_finite() || !z.is_finite() || !x_hint.is_finite() || z.len() <= tol::LINEAR {
            return None;
        }
        let z = z.normalized();
        let x = x_hint - z * x_hint.dot(z);
        if x.len() <= tol::LINEAR {
            return None;
        }
        let x = x.normalized();
        Some(Frame { origin, x, y: z.cross(x), z })
    }

    /// Frame at `origin` with normal `z` and a deterministic X direction: world X projected onto
    /// the plane, or world Y when `z` is close to world X.
    pub fn from_normal(origin: Vec3, z: Vec3) -> Option<Frame> {
        let zn = z.normalized();
        let hint = if zn.x.abs() > 0.9 { Vec3::Y } else { Vec3::X };
        Frame::new(origin, z, hint)
    }

    pub fn origin(&self) -> Vec3 {
        self.origin
    }
    pub fn x(&self) -> Vec3 {
        self.x
    }
    pub fn y(&self) -> Vec3 {
        self.y
    }
    pub fn z(&self) -> Vec3 {
        self.z
    }

    /// The same frame moved to `origin`. `None` if `origin` is not finite.
    pub fn with_origin(&self, origin: Vec3) -> Option<Frame> {
        origin.is_finite().then_some(Frame { origin, ..*self })
    }

    /// Local coordinates to world coordinates.
    pub fn to_world(&self, p: Vec3) -> Vec3 {
        self.origin + self.x * p.x + self.y * p.y + self.z * p.z
    }

    /// World coordinates to local coordinates.
    pub fn to_local(&self, p: Vec3) -> Vec3 {
        let d = p - self.origin;
        Vec3::new(d.dot(self.x), d.dot(self.y), d.dot(self.z))
    }

    /// A point of the frame's XY plane (a sketch point) in world coordinates.
    pub fn plane_point(&self, p: Vec2) -> Vec3 {
        self.to_world(Vec3::new(p.x, p.y, 0.0))
    }
}

/// Axis-aligned bounding box in 3D.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Aabb3 {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb3 {
    /// Box spanning two corners in any order.
    pub fn new(a: Vec3, b: Vec3) -> Aabb3 {
        Aabb3 { min: a.min(b), max: a.max(b) }
    }

    /// Smallest box containing all points; `None` for no points.
    pub fn from_points(points: impl IntoIterator<Item = Vec3>) -> Option<Aabb3> {
        let mut it = points.into_iter();
        let first = it.next()?;
        Some(it.fold(Aabb3 { min: first, max: first }, |b, p| Aabb3 { min: b.min.min(p), max: b.max.max(p) }))
    }

    pub fn size(&self) -> Vec3 {
        self.max - self.min
    }
    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }
    pub fn diagonal(&self) -> f64 {
        self.size().len()
    }
    pub fn union(&self, o: &Aabb3) -> Aabb3 {
        Aabb3 { min: self.min.min(o.min), max: self.max.max(o.max) }
    }
    /// `p` lies inside or within `tol` of the box.
    pub fn contains(&self, p: Vec3, tol: f64) -> bool {
        p.x >= self.min.x - tol
            && p.y >= self.min.y - tol
            && p.z >= self.min.z - tol
            && p.x <= self.max.x + tol
            && p.y <= self.max.y + tol
            && p.z <= self.max.z + tol
    }
    /// Corners agree within `tol`.
    pub fn near(&self, o: &Aabb3, tol: f64) -> bool {
        self.min.near(o.min, tol) && self.max.near(o.max, tol)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn orthonormal(f: &Frame) -> bool {
        let t = 1e-12;
        (f.x().len() - 1.0).abs() < t
            && (f.y().len() - 1.0).abs() < t
            && (f.z().len() - 1.0).abs() < t
            && f.x().dot(f.y()).abs() < t
            && f.y().dot(f.z()).abs() < t
            && f.x().cross(f.y()).near(f.z(), t)
    }

    #[test]
    fn frames_are_right_handed_orthonormal() {
        let normals = [Vec3::Z, -Vec3::Z, Vec3::X, -Vec3::X, Vec3::Y, Vec3::new(1.0, 2.0, 3.0), Vec3::new(0.95, 0.0, 0.1)];
        for n in normals {
            let f = Frame::from_normal(Vec3::new(1.0, -2.0, 3.0), n).unwrap();
            assert!(orthonormal(&f), "{n:?}");
            assert!(f.z().near(n.normalized(), 1e-12));
        }
        assert!(orthonormal(&Frame::WORLD));
    }

    #[test]
    fn frame_round_trip() {
        let f = Frame::new(Vec3::new(5.0, 6.0, 7.0), Vec3::new(0.0, 1.0, 1.0), Vec3::X).unwrap();
        let p = Vec3::new(-3.0, 0.25, 9.0);
        assert!(f.to_local(f.to_world(p)).near(p, 1e-12));
        assert!(f.plane_point(Vec2::new(1.0, 0.0)).near(f.origin() + f.x(), 1e-12));
    }

    #[test]
    fn degenerate_frames_rejected() {
        assert!(Frame::new(Vec3::ZERO, Vec3::ZERO, Vec3::X).is_none());
        assert!(Frame::new(Vec3::ZERO, Vec3::X, Vec3::X * 2.0).is_none());
        assert!(Frame::new(Vec3::new(f64::NAN, 0.0, 0.0), Vec3::Z, Vec3::X).is_none());
        assert!(Frame::new(Vec3::ZERO, Vec3::Z, Vec3::new(f64::INFINITY, 0.0, 0.0)).is_none());
        assert!(Axis::new(Vec3::ZERO, Vec3::ZERO).is_none());
        assert!((Axis::new(Vec3::ZERO, Vec3::new(0.0, 3.0, 4.0)).unwrap().dir().len() - 1.0).abs() < 1e-15);
    }

    #[test]
    fn aabb_basics() {
        let b = Aabb3::new(Vec3::new(2.0, 0.0, 5.0), Vec3::new(0.0, 4.0, 1.0));
        assert_eq!(b.min, Vec3::new(0.0, 0.0, 1.0));
        assert_eq!(b.size(), Vec3::new(2.0, 4.0, 4.0));
        assert!(b.contains(Vec3::new(1.0, 1.0, 1.0), 0.0));
        assert!(!b.contains(Vec3::new(3.0, 1.0, 1.0), 0.5));
        assert!(Aabb3::from_points([]).is_none());
        let p = Aabb3::from_points([Vec3::X, Vec3::Y, -Vec3::Z]).unwrap();
        assert!(p.near(&Aabb3::new(Vec3::new(0.0, 0.0, -1.0), Vec3::new(1.0, 1.0, 0.0)), 0.0));
    }
}
