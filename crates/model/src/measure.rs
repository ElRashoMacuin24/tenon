//! Measuring the part: the size of one face or edge, where one work plane, axis or point is, and the
//! distance and angle between two. The planes, axes and origin of a user coordinate system are
//! measured as work features are (DEC-133).

use serde::{Deserialize, Serialize};
use tenon_geom::{Axis, Frame, Vec3};
use tenon_kernel::{Curve2, CurveKind, Kernel, Loop, Profile, Region, ShapeHandle, SubShape, SurfaceKind, TaggedCurve2};

use crate::FeatureId;
use crate::document::{OriginAxis, OriginPlane};
use crate::regen::{Regen, WorkGeom, ucs_axis, ucs_plane};

/// What of a work feature is meant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkPart {
    /// The work plane, axis or point itself; of a UCS, its origin.
    Whole,
    /// A plane of a UCS.
    Plane(OriginPlane),
    /// An axis of a UCS.
    Axis(OriginAxis),
}

/// A face or edge of a body of a regeneration result, or a work feature.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Entity {
    Face { body: usize, face: u32 },
    Edge { body: usize, edge: u32 },
    Work { feature: FeatureId, part: WorkPart },
}

/// What was measured: labelled values (in mm, mm^2 and degrees) and, between two entities, the
/// nearest points (to draw the distance).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Measurement {
    pub values: Vec<(String, f64, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nearest: Option<(Vec3, Vec3)>,
}

impl Measurement {
    fn push(&mut self, label: &str, v: f64, unit: &str) {
        self.values.push((label.to_owned(), v, unit.to_owned()));
    }
    /// The value with this label.
    pub fn get(&self, label: &str) -> Option<f64> {
        self.values.iter().find(|v| v.0 == label).map(|v| v.1)
    }
}

/// Direction of a planar face's normal or a straight edge, for angles.
enum Dir {
    Normal(Vec3),
    Line(Vec3),
}

/// What is measured, as the kernel or plain geometry has it.
#[derive(Clone, Copy)]
enum Place {
    Sub(SubShape),
    Plane(Frame),
    Line(Axis),
    Point(Vec3),
}

fn place_of(regen: &Regen, e: Entity) -> Result<Place, String> {
    let shape = |b: usize| regen.bodies.get(b).map(|x| x.shape).ok_or_else(|| "that body no longer exists".to_string());
    Ok(match e {
        Entity::Face { body, face } => Place::Sub(SubShape::Face(shape(body)?.face(face))),
        Entity::Edge { body, edge } => Place::Sub(SubShape::Edge(shape(body)?.edge(edge))),
        Entity::Work { feature, part } => match (regen.work.get(&feature), part) {
            (Some(WorkGeom::Plane(f)), WorkPart::Whole) => Place::Plane(*f),
            (Some(WorkGeom::Axis(a)), WorkPart::Whole) => Place::Line(*a),
            (Some(WorkGeom::Point(p)), WorkPart::Whole) => Place::Point(*p),
            (Some(WorkGeom::Ucs(f)), WorkPart::Whole) => Place::Point(f.origin()),
            (Some(WorkGeom::Ucs(f)), WorkPart::Plane(p)) => Place::Plane(ucs_plane(f, p).ok_or("that plane of the UCS is out of range")?),
            (Some(WorkGeom::Ucs(f)), WorkPart::Axis(a)) => Place::Line(ucs_axis(f, a).ok_or("that axis of the UCS is out of range")?),
            (Some(_), _) => return Err(format!("{feature} is not a UCS")),
            (None, _) => return Err(format!("{feature} is not a work feature of the part as it stands")),
        },
    })
}

/// The size of one entity, its direction (for angles) and a prefix for its labels.
fn describe(k: &dyn Kernel, place: Place, m: &mut Measurement, prefix: &str) -> Result<Option<Dir>, String> {
    let err = |x: tenon_kernel::KernelError| x.to_string();
    match place {
        Place::Plane(f) => Ok(Some(Dir::Normal(f.z()))),
        Place::Line(a) => Ok(Some(Dir::Line(a.dir()))),
        Place::Point(p) => {
            for (label, v) in [("X", p.x), ("Y", p.y), ("Z", p.z)] {
                m.push(&format!("{prefix}{label}"), v, "mm");
            }
            Ok(None)
        }
        Place::Sub(SubShape::Face(f)) => {
            let info = k.face_info(f).map_err(err)?;
            m.push(&format!("{prefix}Area"), info.area, "mm^2");
            Ok(match info.surface {
                SurfaceKind::Plane { normal, .. } => Some(Dir::Normal(normal)),
                SurfaceKind::Cylinder { radius, axis } => {
                    m.push(&format!("{prefix}Diameter"), 2.0 * radius, "mm");
                    Some(Dir::Line(axis.dir()))
                }
                SurfaceKind::Sphere { radius, .. } => {
                    m.push(&format!("{prefix}Diameter"), 2.0 * radius, "mm");
                    None
                }
                _ => None,
            })
        }
        Place::Sub(SubShape::Edge(ed)) => {
            let info = k.edge_info(ed).map_err(err)?;
            m.push(&format!("{prefix}Length"), info.length, "mm");
            Ok(match info.curve {
                CurveKind::Line { dir, .. } => Some(Dir::Line(dir)),
                CurveKind::Circle { radius, .. } => {
                    m.push(&format!("{prefix}Diameter"), 2.0 * radius, "mm");
                    None
                }
                _ => None,
            })
        }
        _ => Ok(None),
    }
}

/// The foot of `p` on a line.
fn foot(p: Vec3, a: &Axis) -> Vec3 {
    a.origin() + a.dir() * (p - a.origin()).dot(a.dir())
}

/// The foot of `p` on a plane.
fn drop_on(p: Vec3, f: &Frame) -> Vec3 {
    p - f.z() * (p - f.origin()).dot(f.z())
}

/// The nearest points of two places of plain geometry.
fn nearest_plain(a: Place, b: Place) -> Option<(Vec3, Vec3)> {
    const PARALLEL: f64 = 1e-9;
    Some(match (a, b) {
        (Place::Point(p), Place::Point(q)) => (p, q),
        (Place::Point(p), Place::Plane(f)) => (p, drop_on(p, &f)),
        (Place::Point(p), Place::Line(l)) => (p, foot(p, &l)),
        (Place::Line(l), Place::Plane(f)) => {
            let along = l.dir().dot(f.z());
            if along.abs() < PARALLEL {
                (l.origin(), drop_on(l.origin(), &f))
            } else {
                // They meet.
                let at = l.origin() + l.dir() * ((f.origin() - l.origin()).dot(f.z()) / along);
                (at, at)
            }
        }
        (Place::Line(l), Place::Line(n)) => {
            let w = l.origin() - n.origin();
            let (u, v) = (l.dir(), n.dir());
            let c = u.dot(v);
            let d = 1.0 - c * c;
            if d < PARALLEL {
                (l.origin(), foot(l.origin(), &n))
            } else {
                let s = (c * v.dot(w) - u.dot(w)) / d;
                let t = (v.dot(w) - c * u.dot(w)) / d;
                (l.origin() + u * s, n.origin() + v * t)
            }
        }
        (Place::Plane(f), Place::Plane(g)) => {
            if f.z().cross(g.z()).len() < PARALLEL {
                (f.origin(), drop_on(f.origin(), &g))
            } else {
                // They meet in a line: a point of it.
                let dir = f.z().cross(g.z());
                let (d1, d2, c) = (f.z().dot(f.origin()), g.z().dot(g.origin()), f.z().dot(g.z()));
                let det = dir.dot(dir);
                let at = f.z() * ((d1 - d2 * c) / det) + g.z() * ((d2 - d1 * c) / det);
                (at, at)
            }
        }
        // The other way round.
        (Place::Plane(_) | Place::Line(_), Place::Point(_)) | (Place::Plane(_), Place::Line(_)) => {
            let (p, q) = nearest_plain(b, a)?;
            (q, p)
        }
        _ => return None,
    })
}

/// How thick the thin solid is that stands for an axis or a point when the kernel measures to it.
const THIN: f64 = 1e-3;

/// A solid or face that stands for `place` when the kernel measures from it to `round`, which it
/// must reach past.
fn stand_in(k: &mut dyn Kernel, place: Place, round: tenon_geom::Aabb3) -> Result<ShapeHandle, String> {
    let err = |x: tenon_kernel::KernelError| x.to_string();
    let reach = |at: Vec3| at.dist(round.center()) + round.diagonal() + 1.0;
    match place {
        Place::Point(p) => Ok(k.make_sphere(p, THIN).map_err(err)?.shape),
        Place::Line(a) => {
            let n = reach(a.origin());
            let start = Axis::new(a.origin() - a.dir() * n, a.dir()).ok_or("the axis is out of range")?;
            Ok(k.make_cylinder(&start, THIN, 2.0 * n).map_err(err)?.shape)
        }
        Place::Plane(f) => {
            let n = reach(f.origin());
            let corners = [(-n, -n), (n, -n), (n, n), (-n, n)].map(|(x, y)| tenon_geom::Vec2::new(x, y));
            let curves =
                (0..4).map(|i| TaggedCurve2 { tag: i as u64 + 1, curve: Curve2::Line { start: corners[i], end: corners[(i + 1) % 4] } }).collect();
            let profile = Profile { frame: f, regions: vec![Region { outer: Loop { curves }, holes: vec![] }] };
            Ok(k.make_face(&profile).map_err(err)?.shape)
        }
        Place::Sub(_) => Err("internal: a face or an edge needs no stand-in".into()),
    }
}

/// The nearest point of plain geometry to a point near it.
fn on_plain(place: Place, near: Vec3) -> Vec3 {
    match place {
        Place::Point(p) => p,
        Place::Line(a) => foot(near, &a),
        Place::Plane(f) => drop_on(near, &f),
        Place::Sub(_) => near,
    }
}

/// The box round the body a face or an edge is on.
fn box_of(k: &dyn Kernel, s: SubShape) -> Result<tenon_geom::Aabb3, String> {
    let shape = match s {
        SubShape::Face(f) => f.shape,
        SubShape::Edge(e) => e.shape,
        SubShape::Vertex(v) => v.shape,
        SubShape::Shape(h) => h,
    };
    k.bounding_box(shape).map_err(|e| e.to_string())?.ok_or_else(|| "that body has no size".to_string())
}

/// The shortest distance between two places and the nearest points.
fn between(k: &mut dyn Kernel, a: Place, b: Place) -> Result<(f64, Vec3, Vec3), String> {
    let err = |x: tenon_kernel::KernelError| x.to_string();
    match (a, b) {
        (Place::Sub(x), Place::Sub(y)) => {
            let d = k.min_distance(x, y).map_err(err)?;
            Ok((d.value, d.on_a, d.on_b))
        }
        (Place::Sub(x), plain) | (plain, Place::Sub(x)) => {
            let round = box_of(k, x)?;
            let thing = stand_in(k, plain, round)?;
            let found = k.min_distance(x, SubShape::Shape(thing));
            k.release(thing);
            let d = found.map_err(err)?;
            let (on_sub, on_plain_end) = (d.on_a, on_plain(plain, d.on_b));
            // (Within the stand-in's thickness of it, the two touch.)
            let value = if d.value <= tenon_geom::tol::LINEAR { 0.0 } else { on_sub.dist(on_plain_end) };
            Ok(if matches!(a, Place::Sub(_)) { (value, on_sub, on_plain_end) } else { (value, on_plain_end, on_sub) })
        }
        _ => {
            let (p, q) = nearest_plain(a, b).ok_or("those two cannot be measured to one another")?;
            Ok((p.dist(q), p, q))
        }
    }
}

/// Measures one entity, or two (their sizes, the shortest distance between them, and the angle
/// between planes and straight lines).
pub fn measure(k: &mut dyn Kernel, regen: &Regen, a: Entity, b: Option<Entity>) -> Result<Measurement, String> {
    let mut m = Measurement::default();
    let a = place_of(regen, a)?;
    let Some(b) = b else {
        describe(k, a, &mut m, "")?;
        // A plane or an axis has no size: where it is, and the way it goes.
        let (at, way, name) = match a {
            Place::Plane(f) => (f.origin(), f.z(), "Normal"),
            Place::Line(l) => (l.origin(), l.dir(), "Along"),
            _ => return Ok(m),
        };
        for (label, v) in [("X", at.x), ("Y", at.y), ("Z", at.z)] {
            m.push(label, v, "mm");
        }
        for (label, v) in [("X", way.x), ("Y", way.y), ("Z", way.z)] {
            m.push(&format!("{name} {label}"), v, "");
        }
        return Ok(m);
    };
    let b = place_of(regen, b)?;
    let (value, on_a, on_b) = between(k, a, b)?;
    m.push("Distance", value, "mm");
    let delta = on_b - on_a;
    for (label, v) in [("dX", delta.x), ("dY", delta.y), ("dZ", delta.z)] {
        m.push(label, v.abs(), "mm");
    }
    m.nearest = Some((on_a, on_b));
    let da = describe(k, a, &mut m, "First ")?;
    let db = describe(k, b, &mut m, "Second ")?;
    // Angles: between planes by their normals, between lines by their directions, and between a
    // line and a plane by the line's angle to the plane.
    let angle = match (da, db) {
        (Some(Dir::Normal(n1)), Some(Dir::Normal(n2))) => Some(n1.dot(n2).abs().clamp(0.0, 1.0).acos()),
        (Some(Dir::Line(l1)), Some(Dir::Line(l2))) => Some(l1.dot(l2).abs().clamp(0.0, 1.0).acos()),
        (Some(Dir::Normal(n)), Some(Dir::Line(l))) | (Some(Dir::Line(l)), Some(Dir::Normal(n))) => {
            Some(std::f64::consts::FRAC_PI_2 - n.dot(l).abs().clamp(0.0, 1.0).acos())
        }
        _ => None,
    };
    if let Some(a) = angle {
        m.push("Angle", a.to_degrees(), "deg");
    }
    Ok(m)
}
