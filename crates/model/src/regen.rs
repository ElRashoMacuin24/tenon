//! Regeneration: rebuilding the part from its features through the kernel.

use std::collections::BTreeMap;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tenon_geom::{Aabb3, Axis, Frame, Vec3};
use tenon_kernel::{AngleExtent, BoolOp, Extent, FaceInfo, Kernel, KernelError, Mesh, MeshTol, ShapeHandle, SurfaceKind};
use tenon_sketch::{Sketch, SketchRegion, default_regions, profile, regions};

use crate::FeatureId;
use crate::document::{AxisRef, Document, Extrude, ExtrudeExtent, Feature, FeatureKind, Operation, PlaneRef, RegionSel, Revolve, RevolveAngle};
use crate::naming::{FaceOrigin, FaceRef, Fingerprint, names_of_boolean, names_of_sweep, resolve};

/// A solid body of the part and the names of its faces.
#[derive(Clone, Debug, PartialEq)]
pub struct Body {
    pub shape: ShapeHandle,
    pub names: Vec<Option<FaceOrigin>>,
    /// The feature that created the body.
    pub created_by: FeatureId,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum FeatureStatus {
    Ok,
    Suppressed,
    Error {
        message: String,
    },
    /// After a failed feature, later features are not computed.
    NotComputed,
}

/// Result of a regeneration. Shapes live in the kernel; call [`Regen::release`] when done.
#[derive(Clone, Debug, Default)]
pub struct Regen {
    pub bodies: Vec<Body>,
    pub status: Vec<(FeatureId, FeatureStatus)>,
    pub sketch_frames: BTreeMap<FeatureId, Frame>,
    pub cancelled: bool,
    /// Wall time of the regeneration in milliseconds.
    pub millis: f64,
}

impl Regen {
    pub fn release(&mut self, k: &mut dyn Kernel) {
        for b in self.bodies.drain(..) {
            k.release(b.shape);
        }
    }
    pub fn status_of(&self, id: FeatureId) -> Option<&FeatureStatus> {
        self.status.iter().find(|(f, _)| *f == id).map(|(_, s)| s)
    }
    /// The first failing feature and its message.
    pub fn first_error(&self) -> Option<(FeatureId, &str)> {
        self.status.iter().find_map(|(f, s)| match s {
            FeatureStatus::Error { message } => Some((*f, message.as_str())),
            _ => None,
        })
    }
    fn views(&self) -> Vec<(ShapeHandle, &[Option<FaceOrigin>])> {
        self.bodies.iter().map(|b| (b.shape, b.names.as_slice())).collect()
    }
    /// A persistent reference to face `face` of body `body`, if that face can be referenced.
    pub fn face_ref(&self, body: usize, face: u32, k: &dyn Kernel) -> Result<FaceRef, String> {
        let b = self.bodies.get(body).ok_or("no such body")?;
        let origin = b.names.get(face as usize).copied().flatten().ok_or("this face cannot be referenced yet")?;
        let info = k.face_info(b.shape.face(face)).map_err(|e| e.to_string())?;
        Ok(FaceRef { origin, fingerprint: Fingerprint::of(&info) })
    }
    /// Resolves a reference against this result.
    pub fn resolve(&self, fref: &FaceRef, k: &dyn Kernel) -> Result<(usize, u32), String> {
        resolve(fref, &self.views(), k)
    }
}

/// Feature names for messages.
fn label(doc: &Document, id: FeatureId) -> String {
    doc.feature(id).map_or_else(|| id.to_string(), |f| f.name.clone())
}

struct Ctx<'a> {
    doc: &'a Document,
    k: &'a mut dyn Kernel,
    regen: Regen,
}

fn kerr(e: KernelError) -> String {
    e.to_string()
}

/// Rebuilds the part. Never panics; failures are reported per feature.
pub fn regenerate(doc: &Document, k: &mut dyn Kernel) -> Regen {
    let t0 = Instant::now();
    let mut cx = Ctx { doc, k, regen: Regen::default() };
    let mut failed = false;
    for f in doc.features() {
        let status = if failed {
            FeatureStatus::NotComputed
        } else if f.suppressed {
            FeatureStatus::Suppressed
        } else {
            match cx.step(f) {
                Ok(()) => FeatureStatus::Ok,
                Err(Stop::Cancelled) => {
                    cx.regen.cancelled = true;
                    break;
                }
                Err(Stop::Failed(message)) => {
                    failed = true;
                    FeatureStatus::Error { message }
                }
            }
        };
        cx.regen.status.push((f.id, status));
    }
    let mut regen = cx.regen;
    regen.millis = t0.elapsed().as_secs_f64() * 1000.0;
    regen
}

enum Stop {
    Cancelled,
    Failed(String),
}

impl From<String> for Stop {
    fn from(s: String) -> Self {
        Stop::Failed(s)
    }
}
impl From<&str> for Stop {
    fn from(s: &str) -> Self {
        Stop::Failed(s.to_owned())
    }
}
impl From<KernelError> for Stop {
    fn from(e: KernelError) -> Self {
        match e {
            KernelError::Cancelled => Stop::Cancelled,
            e => Stop::Failed(kerr(e)),
        }
    }
}

fn select<'r>(all: &'r [SketchRegion], sel: &RegionSel) -> Result<Vec<&'r SketchRegion>, String> {
    let chosen: Vec<&SketchRegion> = match sel {
        RegionSel::Default => default_regions(all),
        RegionSel::Keys(keys) => {
            let mut out = Vec::new();
            for key in keys {
                let mut sorted = key.clone();
                sorted.sort();
                sorted.dedup();
                let r = all.iter().find(|r| r.key == sorted).ok_or("a selected profile region no longer exists in the sketch")?;
                out.push(r);
            }
            out
        }
    };
    if chosen.is_empty() {
        return Err("the sketch has no closed profile".into());
    }
    Ok(chosen)
}

impl<'a> Ctx<'a> {
    fn step(&mut self, f: &Feature) -> Result<(), Stop> {
        match &f.kind {
            FeatureKind::Sketch { plane, .. } => {
                let frame = self.plane_frame(plane)?;
                self.regen.sketch_frames.insert(f.id, frame);
                Ok(())
            }
            FeatureKind::Extrude(e) => self.extrude(f.id, e),
            FeatureKind::Revolve(r) => self.revolve(f.id, r),
        }
    }

    fn plane_frame(&mut self, plane: &PlaneRef) -> Result<Frame, Stop> {
        match plane {
            PlaneRef::Origin(p) => Ok(p.frame()),
            PlaneRef::Face(fref) => {
                let name = fref.origin.describe(&label(self.doc, fref.origin.feature()));
                let (b, face) = self.regen.resolve(fref, &*self.k).map_err(|e| format!("{e} ({name})"))?;
                let shape = self.regen.bodies.get(b).ok_or("no such body")?.shape;
                match self.k.face_info(shape.face(face))?.surface {
                    SurfaceKind::Plane { origin, normal } => {
                        // The sketch origin is the part origin projected onto the face's plane.
                        let o = normal * origin.dot(normal);
                        Ok(Frame::from_normal(o, normal).ok_or("the face has no valid normal")?)
                    }
                    _ => Err(format!("{name} is not planar").into()),
                }
            }
        }
    }

    fn sketch_and_frame(&self, id: FeatureId) -> Result<(&'a Sketch, Frame), Stop> {
        let doc: &'a Document = self.doc;
        let sketch = doc.sketch(id).ok_or_else(|| format!("{} is not a sketch", label(doc, id)))?;
        let frame = *self.regen.sketch_frames.get(&id).ok_or_else(|| format!("{} is suppressed or failed", label(self.doc, id)))?;
        Ok((sketch, frame))
    }

    fn target_bbox(&self) -> Option<Aabb3> {
        self.regen.bodies.iter().filter_map(|b| self.k.bounding_box(b.shape).ok().flatten()).reduce(|a, b| a.union(&b))
    }

    fn extrude(&mut self, id: FeatureId, e: &Extrude) -> Result<(), Stop> {
        let (sketch, frame) = self.sketch_and_frame(e.sketch)?;
        let all = regions(sketch);
        let profile = profile(sketch, frame, &select(&all, &e.regions)?);
        let sign = if e.reverse { -1.0 } else { 1.0 };
        let extent = match &e.extent {
            ExtrudeExtent::Distance(d) => Extent::Distance(sign * d),
            ExtrudeExtent::Symmetric(d) => Extent::Symmetric(*d),
            ExtrudeExtent::TwoSided { forward, backward } => {
                if e.reverse {
                    Extent::TwoSided { forward: *backward, backward: *forward }
                } else {
                    Extent::TwoSided { forward: *forward, backward: *backward }
                }
            }
            ExtrudeExtent::ThroughAll => {
                let b = self.target_bbox().ok_or("there is no body to extrude through")?;
                let dir = frame.z() * sign;
                let reach = corners(&b).into_iter().map(|c| (c - frame.origin()).dot(dir)).fold(f64::NEG_INFINITY, f64::max);
                if reach <= 0.0 {
                    return Err("the body is not on that side of the sketch".into());
                }
                Extent::Distance(sign * (reach + 1.0))
            }
        };
        let op = self.k.extrude(&profile, &extent, None)?;
        let faces = self.k.topology(op.shape)?.faces;
        let names = names_of_sweep(&op.history, id, faces);
        self.combine(id, op.shape, names, e.operation)
    }

    fn revolve(&mut self, id: FeatureId, r: &Revolve) -> Result<(), Stop> {
        let (sketch, frame) = self.sketch_and_frame(r.sketch)?;
        let axis = match &r.axis {
            AxisRef::Origin(a) => a.axis(),
            AxisRef::SketchLine(line) => {
                let (a, b) = sketch.line(*line).ok_or("the revolve axis is not a line of the sketch")?;
                let (a3, b3) = (frame.plane_point(a), frame.plane_point(b));
                Axis::new(a3, b3 - a3).ok_or("the revolve axis has no length")?
            }
        };
        let all = regions(sketch);
        let profile = profile(sketch, frame, &select(&all, &r.regions)?);
        let angle = match r.angle {
            RevolveAngle::Full => AngleExtent::Full,
            RevolveAngle::Angle(a) => AngleExtent::Angle(a),
            RevolveAngle::Symmetric(a) => AngleExtent::Symmetric(a),
        };
        let op = self.k.revolve(&profile, &axis, &angle)?;
        let faces = self.k.topology(op.shape)?.faces;
        let names = names_of_sweep(&op.history, id, faces);
        self.combine(id, op.shape, names, r.operation)
    }

    /// Adds a feature's tool solid to the part according to `operation`.
    fn combine(&mut self, id: FeatureId, tool: ShapeHandle, names: Vec<Option<FaceOrigin>>, operation: Operation) -> Result<(), Stop> {
        let kind = match operation {
            Operation::NewBody => None,
            Operation::Join if self.regen.bodies.is_empty() => None,
            Operation::Join => Some(BoolOp::Union),
            Operation::Cut => Some(BoolOp::Cut),
            Operation::Intersect => Some(BoolOp::Intersect),
        };
        let Some(kind) = kind else {
            self.regen.bodies.push(Body { shape: tool, names, created_by: id });
            return Ok(());
        };
        let Some(target) = self.regen.bodies.last() else {
            self.k.release(tool);
            return Err("there is no body to combine with".into());
        };
        let (target_shape, target_names) = (target.shape, target.names.clone());
        let op = match self.k.boolean(kind, target_shape, &[tool]) {
            Ok(op) => op,
            Err(e) => {
                self.k.release(tool);
                return Err(e.into());
            }
        };
        self.k.release(tool);
        let volume = self.k.mass_properties(op.shape, 1.0)?.volume;
        if volume.abs() <= tenon_geom::tol::MIN_SIZE {
            self.k.release(op.shape);
            return Err("the result has no volume left".into());
        }
        let faces = self.k.topology(op.shape)?.faces;
        let new_names = names_of_boolean(&op.history, &[&target_names, &names], faces);
        self.k.release(target_shape);
        if let Some(body) = self.regen.bodies.last_mut() {
            body.shape = op.shape;
            body.names = new_names;
        }
        Ok(())
    }
}

fn corners(b: &Aabb3) -> [Vec3; 8] {
    let (l, h) = (b.min, b.max);
    [
        Vec3::new(l.x, l.y, l.z),
        Vec3::new(h.x, l.y, l.z),
        Vec3::new(l.x, h.y, l.z),
        Vec3::new(h.x, h.y, l.z),
        Vec3::new(l.x, l.y, h.z),
        Vec3::new(h.x, l.y, h.z),
        Vec3::new(l.x, h.y, h.z),
        Vec3::new(h.x, h.y, h.z),
    ]
}

/// Display data of one body.
#[derive(Clone, Debug, PartialEq)]
pub struct BodyView {
    pub mesh: Mesh,
    /// Per face: its persistent name (if any) and geometry.
    pub faces: Vec<(Option<FaceOrigin>, FaceInfo)>,
    pub volume: f64,
    pub bbox: Option<Aabb3>,
}

/// Everything the UI needs to draw a regeneration result (no kernel handles).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene {
    pub bodies: Vec<BodyView>,
    pub status: Vec<(FeatureId, FeatureStatus)>,
    pub sketch_frames: BTreeMap<FeatureId, Frame>,
    pub regen_ms: f64,
    pub mesh_ms: f64,
}

impl Scene {
    pub fn bbox(&self) -> Option<Aabb3> {
        self.bodies.iter().filter_map(|b| b.bbox).reduce(|a, b| a.union(&b))
    }
}

/// Meshes and face data of a regeneration result.
pub fn scene(regen: &Regen, k: &mut dyn Kernel, tol: &MeshTol) -> Result<Scene, String> {
    let t0 = Instant::now();
    let mut bodies = Vec::new();
    for b in &regen.bodies {
        let mesh = k.tessellate(b.shape, tol).map_err(kerr)?;
        let topo = k.topology(b.shape).map_err(kerr)?;
        let mut faces = Vec::with_capacity(topo.faces as usize);
        for i in 0..topo.faces {
            faces.push((b.names.get(i as usize).copied().flatten(), k.face_info(b.shape.face(i)).map_err(kerr)?));
        }
        let volume = k.mass_properties(b.shape, 1.0).map_err(kerr)?.volume;
        bodies.push(BodyView { mesh, faces, volume, bbox: k.bounding_box(b.shape).map_err(kerr)? });
    }
    Ok(Scene {
        bodies,
        status: regen.status.clone(),
        sketch_frames: regen.sketch_frames.clone(),
        regen_ms: regen.millis,
        mesh_ms: t0.elapsed().as_secs_f64() * 1000.0,
    })
}
