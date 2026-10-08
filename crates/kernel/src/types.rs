//! Operation inputs: profiles, extents, feature specifications.
//!
//! These are plain data so the model layer can store and serialise them, and so every backend
//! receives exactly the same description of an operation.

use serde::{Deserialize, Serialize};
use tenon_geom::{Axis, Frame, Vec2, Vec3};

use crate::FaceId;

/// A 2D curve in a profile's plane (sketch coordinates, millimetres).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Curve2 {
    Line {
        start: Vec2,
        end: Vec2,
    },
    /// Counter-clockwise arc from `start_angle` to `end_angle` (radians).
    Arc {
        center: Vec2,
        radius: f64,
        start_angle: f64,
        end_angle: f64,
    },
    Circle {
        center: Vec2,
        radius: f64,
    },
}

/// A profile curve with the caller's tag, usually the id of the sketch entity it came from. The
/// kernel reports the faces each tag generated ([`Origin::ProfileCurve`](crate::Origin)).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaggedCurve2 {
    pub tag: u64,
    pub curve: Curve2,
}

/// A closed chain of curves, joined end to end.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Loop {
    pub curves: Vec<TaggedCurve2>,
}

/// One connected planar region: an outer boundary and holes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Region {
    pub outer: Loop,
    pub holes: Vec<Loop>,
}

/// Planar regions in the XY plane of `frame` (the sketch plane). The sketch layer finds the
/// regions; the kernel only builds faces from them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub frame: Frame,
    pub regions: Vec<Region>,
}

/// How far an extrusion goes, measured along the profile frame's Z.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Extent {
    /// One direction; negative goes along -Z.
    Distance(f64),
    /// Total distance, half on each side of the profile plane.
    Symmetric(f64),
    /// Separate distances along +Z and -Z.
    TwoSided { forward: f64, backward: f64 },
    /// Up to a face of an existing shape.
    ToFace(FaceId),
    /// Through everything, along +Z (or -Z when `reverse`). Used for cuts.
    ThroughAll { reverse: bool },
}

/// How far a revolution turns (radians).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AngleExtent {
    Full,
    Angle(f64),
    Symmetric(f64),
    TwoSided { forward: f64, backward: f64 },
}

/// A 3D curve of a sweep path.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Curve3 {
    Line {
        start: Vec3,
        end: Vec3,
    },
    /// Circular arc through three points.
    Arc {
        start: Vec3,
        mid: Vec3,
        end: Vec3,
    },
}

/// An open or closed chain of 3D curves.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Path3 {
    pub curves: Vec<Curve3>,
}

/// How the profile is oriented along a sweep path.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SweepOrientation {
    /// Follows the path's Frenet frame.
    Frenet,
    /// Keeps the profile's original orientation.
    Fixed,
    /// Keeps the profile's normal angle to this direction.
    Binormal(Vec3),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SweepOpts {
    pub orientation: SweepOrientation,
    /// Solid (true) or surface (false) result.
    pub solid: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LoftOpts {
    pub solid: bool,
    /// Straight (ruled) faces between sections instead of smooth ones.
    pub ruled: bool,
    /// Join the last section back to the first.
    pub closed: bool,
}

/// Chamfer size.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ChamferSpec {
    Equal(f64),
    /// `d1` is measured on `reference`, `d2` on the other face.
    TwoDistances {
        d1: f64,
        d2: f64,
        reference: FaceId,
    },
    /// `distance` on `reference`, then `angle` (radians) from it.
    DistanceAngle {
        distance: f64,
        angle: f64,
        reference: FaceId,
    },
}

/// A drilled hole.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HoleSpec {
    /// Centre of the hole on the start face.
    pub position: Vec3,
    /// Drilling direction, into the material.
    pub direction: Vec3,
    pub diameter: f64,
    pub kind: HoleKind,
    pub depth: HoleDepth,
    /// Included angle of the drill point (radians); `None` for a flat bottom.
    pub tip_angle: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum HoleKind {
    Simple,
    Counterbore {
        diameter: f64,
        depth: f64,
    },
    /// `angle` is the included countersink angle (radians).
    Countersink {
        diameter: f64,
        angle: f64,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum HoleDepth {
    Blind(f64),
    ThroughAll,
}

/// Copies of a shape, fused with the original.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Pattern {
    /// `count1` x `count2` copies along two directions (`count2 = 1` for a single row).
    Rectangular { dir1: Vec3, count1: u32, spacing1: f64, dir2: Vec3, count2: u32, spacing2: f64 },
    /// `count` copies spread over `angle` (radians; 2*pi for a full circle) around `axis`.
    Circular { axis: Axis, count: u32, angle: f64 },
    /// Mirror across the XY plane of `plane`.
    Mirror { plane: Frame },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoolOp {
    Union,
    Cut,
    Intersect,
}

/// A placement change.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Transform {
    Translate(Vec3),
    /// Rotation by `angle` radians around `axis` (right-hand rule).
    Rotate {
        axis: Axis,
        angle: f64,
    },
    /// Mirror across the XY plane of `plane`.
    Mirror {
        plane: Frame,
    },
    /// Uniform scale about `center`.
    Scale {
        center: Vec3,
        factor: f64,
    },
}
