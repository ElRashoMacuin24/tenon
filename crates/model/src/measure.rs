//! Measuring the part: the size of one face or edge, and the distance and angle between two.

use serde::{Deserialize, Serialize};
use tenon_geom::Vec3;
use tenon_kernel::{CurveKind, Kernel, SubShape, SurfaceKind};

use crate::regen::Regen;

/// A face or edge of a body of a regeneration result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Entity {
    Face { body: usize, face: u32 },
    Edge { body: usize, edge: u32 },
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

fn sub(regen: &Regen, e: Entity) -> Result<SubShape, String> {
    let shape = |b: usize| regen.bodies.get(b).map(|x| x.shape).ok_or_else(|| "that body no longer exists".to_string());
    Ok(match e {
        Entity::Face { body, face } => SubShape::Face(shape(body)?.face(face)),
        Entity::Edge { body, edge } => SubShape::Edge(shape(body)?.edge(edge)),
    })
}

/// The size of one entity, its direction (for angles) and a prefix for its labels.
fn describe(k: &dyn Kernel, regen: &Regen, e: Entity, m: &mut Measurement, prefix: &str) -> Result<Option<Dir>, String> {
    let err = |x: tenon_kernel::KernelError| x.to_string();
    match sub(regen, e)? {
        SubShape::Face(f) => {
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
        SubShape::Edge(ed) => {
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

/// Measures one entity, or two (their sizes, the shortest distance between them, and the angle
/// between planar faces and straight edges).
pub fn measure(k: &dyn Kernel, regen: &Regen, a: Entity, b: Option<Entity>) -> Result<Measurement, String> {
    let mut m = Measurement::default();
    let Some(b) = b else {
        describe(k, regen, a, &mut m, "")?;
        return Ok(m);
    };
    let d = k.min_distance(sub(regen, a)?, sub(regen, b)?).map_err(|e| e.to_string())?;
    m.push("Distance", d.value, "mm");
    let delta = d.on_b - d.on_a;
    for (label, v) in [("dX", delta.x), ("dY", delta.y), ("dZ", delta.z)] {
        m.push(label, v.abs(), "mm");
    }
    m.nearest = Some((d.on_a, d.on_b));
    let da = describe(k, regen, a, &mut m, "First ")?;
    let db = describe(k, regen, b, &mut m, "Second ")?;
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
