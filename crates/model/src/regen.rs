//! Regeneration: rebuilding the part from its features through the kernel.

use std::collections::BTreeMap;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tenon_geom::{Aabb3, Axis, Frame, Vec2, Vec3};
use tenon_kernel::{
    AngleExtent, BoolOp, ChamferSpec, Curve2, Extent, FaceInfo, Kernel, KernelError, Loop, MassProps, Mesh, MeshTol, Profile, Region, ShapeHandle,
    SurfaceKind, TaggedCurve2,
};
use tenon_sketch::{EntityId, Sketch, SketchRegion, default_regions, profile, regions};

use crate::FeatureId;
use crate::document::{
    AxisRef, ChamferSize, Document, Extrude, ExtrudeExtent, Feature, FeatureKind, Hole, HoleExtent, HoleType, Operation, PlaneRef, RegionSel,
    Revolve, RevolveAngle,
};
use crate::naming::{
    EdgeFingerprint, EdgeRef, FaceOrigin, FaceRef, Fingerprint, HoleFace, edge_names, names_of_boolean, names_of_modify, names_of_sweep,
    names_of_tagged, resolve, resolve_edge,
};

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
    /// A persistent reference to the one face named `origin` (scripts name faces this way, since
    /// face indices depend on the kernel). Fails when no face or several faces have that name.
    pub fn face_ref_by_origin(&self, origin: FaceOrigin, k: &dyn Kernel) -> Result<FaceRef, String> {
        let mut found = self
            .bodies
            .iter()
            .enumerate()
            .flat_map(|(bi, b)| b.names.iter().enumerate().filter(|(_, n)| **n == Some(origin)).map(move |(fi, _)| (bi, fi)));
        match (found.next(), found.next()) {
            (Some((b, f)), None) => self.face_ref(b, u32::try_from(f).map_err(|_| "too many faces")?, k),
            (None, _) => Err("no face has that origin".into()),
            (Some(_), Some(_)) => Err("several faces have that origin; choose one with `body` and `face`".into()),
        }
    }
    /// Resolves a reference against this result.
    pub fn resolve(&self, fref: &FaceRef, k: &dyn Kernel) -> Result<(usize, u32), String> {
        resolve(fref, &self.views(), k)
    }
    /// A persistent reference to edge `edge` of body `body`, if both of its faces are named.
    pub fn edge_ref(&self, body: usize, edge: u32, k: &dyn Kernel) -> Result<EdgeRef, String> {
        let b = self.bodies.get(body).ok_or("no such body")?;
        let topo = k.topology(b.shape).map_err(|e| e.to_string())?;
        let adj = topo.edge_faces.get(edge as usize).ok_or("no such edge")?;
        let [x, y] = edge_names(&b.names, adj).ok_or("this edge cannot be referenced yet")?;
        let info = k.edge_info(b.shape.edge(edge)).map_err(|e| e.to_string())?;
        Ok(EdgeRef::new(x, y, EdgeFingerprint::of(&info)))
    }
    /// Resolves an edge reference against this result: `(body, edge)`.
    pub fn resolve_edge(&self, eref: &EdgeRef, k: &dyn Kernel) -> Result<(usize, u32), String> {
        resolve_edge(eref, &self.views(), k)
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
            FeatureKind::Fillet(fi) => {
                let (bi, edges) = self.resolve_edges(&fi.edges)?;
                let shape = self.body_shape(bi)?;
                let op = self.k.fillet(shape, &edges.iter().map(|e| shape.edge(*e)).collect::<Vec<_>>(), fi.radius)?;
                self.replace_body(f.id, bi, op)
            }
            FeatureKind::Chamfer(c) => {
                let (bi, edges) = self.resolve_edges(&c.edges)?;
                let shape = self.body_shape(bi)?;
                let ids: Vec<_> = edges.iter().map(|e| shape.edge(*e)).collect();
                let spec = match &c.size {
                    ChamferSize::Equal(d) => ChamferSpec::Equal(*d),
                    ChamferSize::TwoDistances { d1, d2, reference } => {
                        ChamferSpec::TwoDistances { d1: *d1, d2: *d2, reference: shape.face(self.face_on(bi, reference)?) }
                    }
                    ChamferSize::DistanceAngle { distance, angle, reference } => {
                        ChamferSpec::DistanceAngle { distance: *distance, angle: *angle, reference: shape.face(self.face_on(bi, reference)?) }
                    }
                };
                let op = self.k.chamfer(shape, &ids, &spec)?;
                self.replace_body(f.id, bi, op)
            }
            FeatureKind::Shell(s) => {
                // The body: the one holding the faces to remove, else the last one.
                let mut bi = self.regen.bodies.len().checked_sub(1).ok_or("there is no body to shell")?;
                let mut faces = Vec::new();
                for (i, fref) in s.remove.iter().enumerate() {
                    let (b, face) = self.regen.resolve(fref, &*self.k).map_err(|e| self.describe_ref(fref, &e))?;
                    if i > 0 && b != bi {
                        return Err("the faces to remove are on different bodies".into());
                    }
                    bi = b;
                    faces.push(face);
                }
                let shape = self.body_shape(bi)?;
                let t = if s.outside { -s.thickness } else { s.thickness };
                let op = self.k.shell(shape, &faces.iter().map(|f| shape.face(*f)).collect::<Vec<_>>(), t)?;
                self.replace_body(f.id, bi, op)
            }
            FeatureKind::Hole(h) => self.hole(f.id, h),
        }
    }

    /// Each hole is its half cross-section revolved about the hole axis; all are cut at once.
    fn hole(&mut self, id: FeatureId, h: &Hole) -> Result<(), Stop> {
        h.check()?;
        let (sketch, frame) = self.sketch_and_frame(h.sketch)?;
        let dir = if h.reverse { frame.z() } else { -frame.z() };
        let bbox = self.target_bbox().ok_or("there is no body to drill")?;
        let mut tools: Vec<(ShapeHandle, Vec<Option<FaceOrigin>>)> = Vec::new();
        let mut made = || -> Result<(), Stop> {
            for p in &h.points {
                let at = sketch.point(*p).ok_or_else(|| format!("hole centre point {} no longer exists in {}", p.0, label(self.doc, h.sketch)))?;
                let origin = frame.plane_point(at);
                let (depth, tip) = match h.extent {
                    HoleExtent::Distance(d) => (d, h.tip_angle),
                    HoleExtent::ThroughAll => {
                        let reach = corners(&bbox).into_iter().map(|c| (c - origin).dot(dir)).fold(f64::NEG_INFINITY, f64::max);
                        if reach <= 0.0 {
                            return Err("the body is not on that side of the sketch".into());
                        }
                        (reach + 1.0, None)
                    }
                };
                let section = hole_section(h, depth, tip, *p)?;
                let curves = (0..section.len())
                    .map(|i| TaggedCurve2 { tag: section[i].1, curve: Curve2::Line { start: section[i].0, end: section[(i + 1) % section.len()].0 } })
                    .collect();
                // Profile x is radial, y runs down the hole.
                let pf = Frame::new(origin, frame.x().cross(dir), frame.x()).ok_or("the hole axis is not valid")?;
                let profile = Profile { frame: pf, regions: vec![Region { outer: Loop { curves }, holes: vec![] }] };
                let axis = Axis::new(origin, dir).ok_or("the hole axis is not valid")?;
                let op = self.k.revolve(&profile, &axis, &AngleExtent::Full)?;
                let faces = match self.k.topology(op.shape) {
                    Ok(t) => t.faces,
                    Err(e) => {
                        self.k.release(op.shape);
                        return Err(e.into());
                    }
                };
                tools.push((op.shape, names_of_tagged(&op.history, id, faces)));
            }
            Ok(())
        };
        if let Err(e) = made() {
            for (t, _) in tools {
                self.k.release(t);
            }
            return Err(e);
        }
        self.combine(id, tools, Operation::Cut)
    }

    fn body_shape(&self, bi: usize) -> Result<ShapeHandle, Stop> {
        Ok(self.regen.bodies.get(bi).ok_or("no such body")?.shape)
    }

    /// "the referenced face no longer exists (end face of Extrusion1)".
    fn describe_ref(&self, fref: &FaceRef, err: &str) -> String {
        format!("{err} ({})", fref.origin.describe(&label(self.doc, fref.origin.feature())))
    }

    /// The face a reference means; it must be on body `bi`.
    fn face_on(&self, bi: usize, fref: &FaceRef) -> Result<u32, Stop> {
        let (b, face) = self.regen.resolve(fref, &*self.k).map_err(|e| self.describe_ref(fref, &e))?;
        if b != bi {
            return Err("the reference face is on another body".into());
        }
        Ok(face)
    }

    /// The body every edge is on, and the edges' indices there.
    fn resolve_edges(&self, edges: &[EdgeRef]) -> Result<(usize, Vec<u32>), Stop> {
        if edges.is_empty() {
            return Err("select at least one edge".into());
        }
        let views = self.regen.views();
        let mut body = None;
        let mut out = Vec::with_capacity(edges.len());
        for e in edges {
            let (b, i) = resolve_edge(e, &views, &*self.k).map_err(|m| {
                let [a, c] = &e.faces;
                format!(
                    "{m} (the edge between the {} and the {})",
                    a.describe(&label(self.doc, a.feature())),
                    c.describe(&label(self.doc, c.feature()))
                )
            })?;
            if body.is_some_and(|x| x != b) {
                return Err("the edges are on different bodies".into());
            }
            body = Some(b);
            out.push(i);
        }
        out.sort_unstable();
        out.dedup();
        Ok((body.unwrap_or(0), out))
    }

    /// Puts the result of an operation on body `bi` in its place, naming its faces.
    fn replace_body(&mut self, id: FeatureId, bi: usize, op: tenon_kernel::Op) -> Result<(), Stop> {
        let Some(old) = self.regen.bodies.get(bi).cloned() else {
            self.k.release(op.shape);
            return Err("no such body".into());
        };
        let names = match (self.k.topology(old.shape), self.k.topology(op.shape)) {
            (Ok(topo), Ok(new)) => names_of_modify(&op.history, &old.names, &topo, id, new.faces),
            (Err(e), _) | (_, Err(e)) => {
                self.k.release(op.shape);
                return Err(e.into());
            }
        };
        self.k.release(old.shape);
        if let Some(body) = self.regen.bodies.get_mut(bi) {
            body.shape = op.shape;
            body.names = names;
        }
        Ok(())
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
        self.combine(id, vec![(op.shape, names)], e.operation)
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
        self.combine(id, vec![(op.shape, names)], r.operation)
    }

    /// Adds a feature's tool solids to the part according to `operation`.
    fn combine(&mut self, id: FeatureId, tools: Vec<(ShapeHandle, Vec<Option<FaceOrigin>>)>, operation: Operation) -> Result<(), Stop> {
        let kind = match operation {
            Operation::NewBody => None,
            Operation::Join if self.regen.bodies.is_empty() => None,
            Operation::Join => Some(BoolOp::Union),
            Operation::Cut => Some(BoolOp::Cut),
            Operation::Intersect => Some(BoolOp::Intersect),
        };
        let Some(kind) = kind else {
            for (shape, names) in tools {
                self.regen.bodies.push(Body { shape, names, created_by: id });
            }
            return Ok(());
        };
        let shapes: Vec<ShapeHandle> = tools.iter().map(|t| t.0).collect();
        let Some(target) = self.regen.bodies.last() else {
            for s in shapes {
                self.k.release(s);
            }
            return Err("there is no body to combine with".into());
        };
        let (target_shape, target_names) = (target.shape, target.names.clone());
        let op = self.k.boolean(kind, target_shape, &shapes);
        for s in shapes {
            self.k.release(s);
        }
        let op = op?;
        let volume = self.k.mass_properties(op.shape, 1.0)?.volume;
        if volume.abs() <= tenon_geom::tol::MIN_SIZE {
            self.k.release(op.shape);
            return Err("the result has no volume left".into());
        }
        let faces = self.k.topology(op.shape)?.faces;
        let inputs: Vec<&[Option<FaceOrigin>]> = std::iter::once(target_names.as_slice()).chain(tools.iter().map(|t| t.1.as_slice())).collect();
        let new_names = names_of_boolean(&op.history, &inputs, faces);
        self.k.release(target_shape);
        if let Some(body) = self.regen.bodies.last_mut() {
            body.shape = op.shape;
            body.names = new_names;
        }
        Ok(())
    }
}

/// Half cross-section of one hole as a closed polygon in (radius, depth) coordinates, each corner
/// with the name source of the segment that starts there. The last segment runs up the axis and
/// makes no face.
fn hole_section(h: &Hole, depth: f64, tip: Option<f64>, point: EntityId) -> Result<Vec<(Vec2, u64)>, String> {
    let r = h.diameter / 2.0;
    let tag = |f: HoleFace| f.source(point);
    let mut pts = Vec::with_capacity(7);
    match h.kind {
        HoleType::Simple => {
            pts.push((Vec2::new(0.0, 0.0), tag(HoleFace::Top)));
            pts.push((Vec2::new(r, 0.0), tag(HoleFace::Wall)));
        }
        HoleType::Counterbore { diameter, depth: bore } => {
            if bore >= depth {
                return Err("the counterbore is deeper than the hole".into());
            }
            pts.push((Vec2::new(0.0, 0.0), tag(HoleFace::Top)));
            pts.push((Vec2::new(diameter / 2.0, 0.0), tag(HoleFace::BoreWall)));
            pts.push((Vec2::new(diameter / 2.0, bore), tag(HoleFace::BoreFloor)));
            pts.push((Vec2::new(r, bore), tag(HoleFace::Wall)));
        }
        HoleType::Countersink { diameter, angle } => {
            let sink = (diameter / 2.0 - r) / (angle / 2.0).tan();
            if sink >= depth {
                return Err("the countersink is deeper than the hole".into());
            }
            pts.push((Vec2::new(0.0, 0.0), tag(HoleFace::Top)));
            pts.push((Vec2::new(diameter / 2.0, 0.0), tag(HoleFace::Sink)));
            pts.push((Vec2::new(r, sink), tag(HoleFace::Wall)));
        }
    }
    match tip {
        Some(a) => {
            pts.push((Vec2::new(r, depth), tag(HoleFace::Point)));
            pts.push((Vec2::new(0.0, depth + r / (a / 2.0).tan()), 0));
        }
        None => {
            pts.push((Vec2::new(r, depth), tag(HoleFace::Bottom)));
            pts.push((Vec2::new(0.0, depth), 0));
        }
    }
    Ok(pts)
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
    /// Mass properties at unit density (mass = volume).
    pub mass: MassProps,
    pub bbox: Option<Aabb3>,
    /// Per edge: the names of its two faces (if both are named) and its geometry.
    pub edges: Vec<(Option<[FaceOrigin; 2]>, EdgeFingerprint)>,
}

impl BodyView {
    /// A persistent reference to edge `edge`, if both its faces are named.
    pub fn edge_ref(&self, edge: u32) -> Option<EdgeRef> {
        let (names, fp) = self.edges.get(edge as usize)?;
        let [a, b] = (*names)?;
        Some(EdgeRef::new(a, b, fp.clone()))
    }
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
        let mut edges = Vec::with_capacity(topo.edges as usize);
        for (i, adj) in topo.edge_faces.iter().enumerate() {
            let info = k.edge_info(b.shape.edge(u32::try_from(i).map_err(|_| "too many edges")?)).map_err(kerr)?;
            edges.push((edge_names(&b.names, adj), EdgeFingerprint::of(&info)));
        }
        let mass = k.mass_properties(b.shape, 1.0).map_err(kerr)?;
        bodies.push(BodyView { mesh, faces, edges, volume: mass.volume, mass, bbox: k.bounding_box(b.shape).map_err(kerr)? });
    }
    Ok(Scene {
        bodies,
        status: regen.status.clone(),
        sketch_frames: regen.sketch_frames.clone(),
        regen_ms: regen.millis,
        mesh_ms: t0.elapsed().as_secs_f64() * 1000.0,
    })
}
