//! What a relationship's geometry is, in its part's coordinates: a plane, a line, a point or a
//! circle, found by persistent name in the part's regenerated [`Scene`] (no kernel needed).

use tenon_geom::{Frame, Vec3, tol};
use tenon_kernel::{CurveKind, SurfaceKind};
use tenon_model::{EdgeFingerprint, Fingerprint, Scene, WorkGeom};

use crate::math::M3;
use crate::model::Geom;

/// Geometry a relationship works with. Normals of faces point out of the material.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Prim {
    Plane {
        point: Vec3,
        normal: Vec3,
    },
    Line {
        point: Vec3,
        dir: Vec3,
    },
    Point(Vec3),
    /// A circular edge: its centre, the axis (pointing out of the material of the planar face
    /// it bounds, when there is one) and radius.
    Circle {
        center: Vec3,
        normal: Vec3,
        radius: f64,
    },
}

impl Prim {
    /// The same geometry moved by a placement (rotation `r`, then translation `t`).
    pub fn placed(&self, r: &M3, t: Vec3) -> Prim {
        match *self {
            Prim::Plane { point, normal } => Prim::Plane { point: r.apply(point) + t, normal: r.apply(normal) },
            Prim::Line { point, dir } => Prim::Line { point: r.apply(point) + t, dir: r.apply(dir) },
            Prim::Point(p) => Prim::Point(r.apply(p) + t),
            Prim::Circle { center, normal, radius } => Prim::Circle { center: r.apply(center) + t, normal: r.apply(normal), radius },
        }
    }
    /// A point of the geometry.
    pub fn point(&self) -> Vec3 {
        match *self {
            Prim::Plane { point, .. } | Prim::Line { point, .. } | Prim::Point(point) => point,
            Prim::Circle { center, .. } => center,
        }
    }
    /// The plane normal, line direction or circle axis.
    pub fn direction(&self) -> Option<Vec3> {
        match *self {
            Prim::Plane { normal, .. } | Prim::Circle { normal, .. } => Some(normal),
            Prim::Line { dir, .. } => Some(dir),
            Prim::Point(_) => None,
        }
    }
    /// The frame a joint takes from this geometry: its point, Z along its direction (world Z
    /// for a point), X chosen from Z alone so the same geometry always gives the same frame.
    pub fn joint_frame(&self) -> Frame {
        let z = self.direction().unwrap_or(Vec3::Z);
        Frame::from_normal(self.point(), z).unwrap_or(Frame::WORLD)
    }
    pub fn kind(&self) -> &'static str {
        match self {
            Prim::Plane { .. } => "plane",
            Prim::Line { .. } => "axis",
            Prim::Point(_) => "point",
            Prim::Circle { .. } => "circle",
        }
    }
}

/// The face of `scene` a reference means: `(body, face)`.
pub fn find_face(scene: &Scene, face: &tenon_model::FaceRef) -> Result<(usize, usize), String> {
    let mut best: Option<(f64, usize, usize)> = None;
    for (bi, b) in scene.bodies.iter().enumerate() {
        for (fi, (name, info)) in b.faces.iter().enumerate() {
            if *name != Some(face.origin) {
                continue;
            }
            let d = Fingerprint::of(info).distance(&face.fingerprint);
            if best.is_none_or(|x| d < x.0) {
                best = Some((d, bi, fi));
            }
        }
    }
    best.map(|(_, b, f)| (b, f)).ok_or_else(|| "the referenced face no longer exists".into())
}

/// The edge of `scene` a reference means: `(body, edge)`.
pub fn find_edge(scene: &Scene, edge: &tenon_model::EdgeRef) -> Result<(usize, usize), String> {
    let mut best: Option<(f64, usize, usize)> = None;
    for (bi, b) in scene.bodies.iter().enumerate() {
        for (ei, (names, fp)) in b.edges.iter().enumerate() {
            if *names != Some(edge.faces) {
                continue;
            }
            let d = EdgeFingerprint::distance(fp, &edge.fingerprint);
            if best.is_none_or(|x| d < x.0) {
                best = Some((d, bi, ei));
            }
        }
    }
    best.map(|(_, b, e)| (b, e)).ok_or_else(|| "the referenced edge no longer exists".into())
}

/// The plane, axis or point of face `face` of body `body`.
pub fn face_prim(scene: &Scene, body: usize, face: usize) -> Result<Prim, String> {
    let (_, info) = scene.bodies.get(body).and_then(|b| b.faces.get(face)).ok_or("no such face")?;
    // A point on an axis near the face, so frames and pickers sit by the face.
    let near_axis = |axis: &tenon_geom::Axis| axis.origin() + axis.dir() * (info.centroid - axis.origin()).dot(axis.dir());
    Ok(match &info.surface {
        // The centroid lies on the plane; joints take their origin there.
        SurfaceKind::Plane { normal, .. } => Prim::Plane { point: info.centroid, normal: *normal },
        SurfaceKind::Cylinder { axis, .. } | SurfaceKind::Cone { axis, .. } | SurfaceKind::Torus { axis, .. } => {
            Prim::Line { point: near_axis(axis), dir: axis.dir() }
        }
        SurfaceKind::Sphere { center, .. } => Prim::Point(*center),
        _ => return Err("this face has no plane, axis or centre (pick a planar, cylindrical, conical or spherical face)".into()),
    })
}

/// The line or circle of edge `edge` of body `body`.
pub fn edge_prim(scene: &Scene, body: usize, edge: usize) -> Result<Prim, String> {
    let b = scene.bodies.get(body).ok_or("no such body")?;
    let curve = b.curves.get(edge).ok_or("no such edge")?;
    Ok(match curve {
        CurveKind::Line { origin, dir } => Prim::Line { point: *origin, dir: dir.normalized() },
        CurveKind::Circle { axis, radius } => {
            // Point the axis out of the material of a planar face the circle bounds.
            let names = b.edges.get(edge).and_then(|(n, _)| *n);
            let outward = names.and_then(|[x, y]| {
                b.faces.iter().find_map(|(name, info)| match &info.surface {
                    SurfaceKind::Plane { normal, .. } if (*name == Some(x) || *name == Some(y)) && normal.cross(axis.dir()).len() < tol::UNIT => {
                        Some(*normal)
                    }
                    _ => None,
                })
            });
            let normal = match outward {
                Some(n) if n.dot(axis.dir()) < 0.0 => -axis.dir(),
                _ => axis.dir(),
            };
            Prim::Circle { center: axis.origin(), normal, radius: *radius }
        }
        _ => return Err("this edge is neither straight nor circular".into()),
    })
}

/// The geometry a [`Geom`] means in its part (`scene`; `None` for the assembly's own origin).
pub fn resolve(geom: &Geom, scene: Option<&Scene>) -> Result<Prim, String> {
    match geom {
        Geom::Plane { plane } => {
            let f = plane.frame();
            Ok(Prim::Plane { point: f.origin(), normal: f.z() })
        }
        Geom::Axis { axis } => {
            let a = axis.axis();
            Ok(Prim::Line { point: a.origin(), dir: a.dir() })
        }
        Geom::Origin => Ok(Prim::Point(Vec3::ZERO)),
        Geom::Face { face } => {
            let scene = scene.ok_or("the part's geometry is not available")?;
            let (b, f) = find_face(scene, face)?;
            face_prim(scene, b, f)
        }
        Geom::Edge { edge } => {
            let scene = scene.ok_or("the part's geometry is not available")?;
            let (b, e) = find_edge(scene, edge)?;
            edge_prim(scene, b, e)
        }
        Geom::Work { feature } => {
            let scene = scene.ok_or("the part's geometry is not available")?;
            match scene.work.iter().find(|(id, _)| id == feature).map(|(_, g)| *g) {
                Some(WorkGeom::Plane(f)) => Ok(Prim::Plane { point: f.origin(), normal: f.z() }),
                Some(WorkGeom::Axis(a)) => Ok(Prim::Line { point: a.origin(), dir: a.dir() }),
                Some(WorkGeom::Point(p)) => Ok(Prim::Point(p)),
                None => Err(format!("work feature {feature} is not in the part")),
            }
        }
    }
}
