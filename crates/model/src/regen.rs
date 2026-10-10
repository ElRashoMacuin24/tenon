//! Regeneration: rebuilding the part from its features through the kernel.

use std::collections::BTreeMap;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tenon_geom::{Aabb3, Axis, Frame, Vec2, Vec3};
use tenon_kernel::{
    AngleExtent, BoolOp, ChamferSpec, Curve2, Curve3, CurveKind, Extent, FaceInfo, Kernel, KernelError, LoftOpts, Loop, MassProps, Mesh, MeshTol,
    Path3, Profile, Region, ShapeHandle, SubShape, SurfaceKind, SweepOpts, SweepOrientation, TaggedCurve2, Transform,
};
use tenon_sketch::{EntityId, Sketch, SketchRegion, default_regions, profile, regions};

use crate::FeatureId;
use crate::document::{
    AxisRef, AxisSel, ChamferSize, Coil, Combine, DirectionRef, Document, Draft, Extrude, ExtrudeExtent, Feature, FeatureKind, Hole, HoleExtent,
    HoleType, Loft, Operation, PlaneRef, RegionSel, Revolve, RevolveAngle, Rib, RibExtent, Split, SplitKeep, Sweep, Thread, ThreadLength, WorkAxis,
    WorkPlane, WorkPoint,
};
use crate::naming::{
    CapEnd, EdgeFingerprint, EdgeRef, FaceOrigin, FaceRef, Fingerprint, HoleFace, edge_names, names_of_boolean, names_of_modify, names_of_sweep,
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
    /// Below the End of Part marker.
    RolledBack,
}

/// Result of a regeneration. Shapes live in the kernel; call [`Regen::release`] when done.
#[derive(Clone, Debug, Default)]
pub struct Regen {
    pub bodies: Vec<Body>,
    pub status: Vec<(FeatureId, FeatureStatus)>,
    pub sketch_frames: BTreeMap<FeatureId, Frame>,
    /// The solids of features that a later pattern or mirror copies, each with how it combined
    /// with the part.
    pub tools: BTreeMap<FeatureId, Vec<Tool>>,
    /// Work planes, axes and points.
    pub work: BTreeMap<FeatureId, WorkGeom>,
    /// The threads on the part, cosmetic and modelled.
    pub threads: Vec<ThreadMark>,
    pub cancelled: bool,
    /// Wall time of the regeneration in milliseconds.
    pub millis: f64,
}

/// A thread on the part: what a Thread feature made of its face.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThreadMark {
    pub feature: FeatureId,
    /// The threaded face (for a modelled thread, what the groove left of it).
    pub face: FaceRef,
    /// "M8x1.25".
    pub designation: String,
    pub pitch: f64,
    /// The major diameter: a shaft's own, a hole's plus the thread's depth both sides.
    pub diameter: f64,
    /// In a hole.
    pub internal: bool,
    pub length: f64,
    /// Where on the axis the thread starts, and which way it runs.
    pub start: Vec3,
    pub direction: Vec3,
    pub left: bool,
    /// The groove is cut into the part.
    pub modelled: bool,
}

/// Where a work feature is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WorkGeom {
    Plane(Frame),
    Axis(Axis),
    Point(Vec3),
}

/// `v` turned `angle` radians about the unit direction `k` (right-hand rule).
fn rotate(v: Vec3, k: Vec3, angle: f64) -> Vec3 {
    let (s, c) = angle.sin_cos();
    v * c + k.cross(v) * s + k * (k.dot(v) * (1.0 - c))
}

/// A feature's solid before it was combined with the part.
#[derive(Clone, Debug, PartialEq)]
pub struct Tool {
    pub operation: Operation,
    pub shape: ShapeHandle,
    pub names: Vec<Option<FaceOrigin>>,
}

impl Regen {
    pub fn release(&mut self, k: &mut dyn Kernel) {
        for b in self.bodies.drain(..) {
            k.release(b.shape);
        }
        for t in std::mem::take(&mut self.tools).into_values().flatten() {
            k.release(t.shape);
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
    /// Features some pattern or mirror copies: their solids are kept.
    copied: std::collections::BTreeSet<FeatureId>,
}

fn kerr(e: KernelError) -> String {
    e.to_string()
}

/// Regeneration state kept between runs, so an edit recomputes only from the first feature it
/// changes: the part as it stood just before that feature is kept as a checkpoint.
#[derive(Debug, Default)]
pub struct RegenCache {
    /// Prefix hashes of the document regenerated last: `last[i]` covers its first `i` features.
    last: Vec<u64>,
    /// The state after the first `.0` features, and the hash of that prefix.
    checkpoint: Option<(usize, u64, Regen)>,
    /// How many features the last run took from the checkpoint (0: computed them all).
    pub resumed_at: usize,
}

impl RegenCache {
    pub fn release(&mut self, k: &mut dyn Kernel) {
        if let Some((_, _, mut r)) = self.checkpoint.take() {
            r.release(k);
        }
        self.last.clear();
    }
}

/// Hashes of the history prefixes: element `i` covers the first `i` features. A feature's hash
/// covers its definition, whether it is rolled back, and whether a pattern copies it (which
/// decides whether its solid is kept).
fn prefix_hashes(doc: &Document, copied: &std::collections::BTreeSet<FeatureId>) -> Vec<u64> {
    use std::hash::{Hash, Hasher};
    let end = doc.end_of_part();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let mut out = Vec::with_capacity(doc.features().len() + 1);
    out.push(h.finish());
    for (i, f) in doc.features().iter().enumerate() {
        serde_json::to_string(f).unwrap_or_default().hash(&mut h);
        (i >= end).hash(&mut h);
        copied.contains(&f.id).hash(&mut h);
        out.push(h.finish());
    }
    out
}

impl Regen {
    /// A copy holding second handles to its shapes (released separately).
    fn duplicate(&self, k: &mut dyn Kernel) -> Result<Regen, KernelError> {
        let mut r = self.clone();
        let mut made = Vec::new();
        let shapes = r.bodies.iter_mut().map(|b| &mut b.shape).chain(r.tools.values_mut().flatten().map(|t| &mut t.shape));
        for s in shapes {
            match k.duplicate(*s) {
                Ok(d) => {
                    *s = d;
                    made.push(d);
                }
                Err(e) => {
                    for m in made {
                        k.release(m);
                    }
                    return Err(e);
                }
            }
        }
        Ok(r)
    }
}

/// Rebuilds the part. Never panics; failures are reported per feature.
pub fn regenerate(doc: &Document, k: &mut dyn Kernel) -> Regen {
    regenerate_with(doc, k, None)
}

/// Rebuilds the part, resuming from `cache`'s checkpoint when the history before it is
/// unchanged, and leaving a checkpoint just before the first feature this document changed.
pub fn regenerate_with(doc: &Document, k: &mut dyn Kernel, mut cache: Option<&mut RegenCache>) -> Regen {
    let t0 = Instant::now();
    let copied: std::collections::BTreeSet<FeatureId> = doc.features().iter().flat_map(|f| f.kind.copies().iter().copied()).collect();
    let hashes = if cache.is_some() { prefix_hashes(doc, &copied) } else { Vec::new() };
    // Resume from the checkpoint if the features before it are the same.
    let mut begin = 0;
    let mut start = Regen::default();
    if let Some(c) = cache.as_deref_mut()
        && let Some((at, hash, state)) = &c.checkpoint
        && hashes.get(*at) == Some(hash)
        && let Ok(copy) = state.duplicate(k)
    {
        begin = *at;
        start = copy;
    }
    // Where this document first differs from the last one: the next checkpoint goes there.
    let unchanged = cache.as_deref().map_or(0, |c| c.last.iter().zip(&hashes).take_while(|(a, b)| a == b).count().saturating_sub(1));
    // (Nothing changed: the checkpoint stays where it is useful.)
    let snapshot_at = (cache.is_some() && unchanged > begin && unchanged < doc.features().len()).then_some(unchanged);
    let mut failed = start.status.iter().any(|(_, s)| matches!(s, FeatureStatus::Error { .. }));
    let mut cx = Ctx { doc, k, regen: start, copied };
    let end = doc.end_of_part();
    for (i, f) in doc.features().iter().enumerate().skip(begin) {
        if Some(i) == snapshot_at
            && !failed
            && let Some(c) = cache.as_deref_mut()
            && let Ok(copy) = cx.regen.duplicate(cx.k)
            && let Some((_, _, mut old)) = c.checkpoint.replace((i, hashes[i], copy))
        {
            old.release(cx.k);
        }
        let status = if i >= end {
            FeatureStatus::RolledBack
        } else if failed {
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
                Err(Stop::Kernel(e)) => {
                    failed = true;
                    FeatureStatus::Error { message: crate::explain::explain(&f.kind, &e) }
                }
            }
        };
        cx.regen.status.push((f.id, status));
    }
    let mut regen = cx.regen;
    if let Some(c) = cache
        && !regen.cancelled
    {
        c.last = hashes;
        c.resumed_at = begin;
    }
    regen.millis = t0.elapsed().as_secs_f64() * 1000.0;
    regen
}

enum Stop {
    Cancelled,
    /// A message already in plain words.
    Failed(String),
    /// The kernel failed: explained for the feature (`explain`).
    Kernel(KernelError),
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
            e => Stop::Kernel(e),
        }
    }
}

fn select<'r>(sketch: &Sketch, all: &'r [SketchRegion], sel: &RegionSel) -> Result<Vec<&'r SketchRegion>, String> {
    let chosen: Vec<&SketchRegion> = match sel {
        RegionSel::Default => default_regions(all),
        RegionSel::Keys(keys) => {
            let mut out: Vec<&SketchRegion> = Vec::new();
            for key in keys {
                let found = tenon_sketch::find(sketch, all, key);
                if found.is_empty() {
                    return Err("a selected profile region no longer exists in the sketch".into());
                }
                for r in found {
                    if !out.iter().any(|o| std::ptr::eq(*o, r)) {
                        out.push(r);
                    }
                }
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
                // A kernel can "build" a shell that hollows nothing (walls thicker than the part
                // allows): that is a failure, never a silent success.
                let volumes = self.k.mass_properties(shape, 1.0).and_then(|a| Ok((a.volume, self.k.mass_properties(op.shape, 1.0)?.volume)));
                match volumes {
                    Ok((before, after)) if (after - before).abs() > tenon_geom::tol::MEASURE_REL * before.abs() => {}
                    Ok(_) => {
                        self.k.release(op.shape);
                        return Err(format!(
                            "The part could not be hollowed out with {} mm walls: the kernel left it solid. The walls are probably too thick for it: try thinner walls.",
                            crate::explain::mm(s.thickness)
                        )
                        .into());
                    }
                    Err(e) => {
                        self.k.release(op.shape);
                        return Err(e.into());
                    }
                }
                self.replace_body(f.id, bi, op)
            }
            FeatureKind::Hole(h) => self.hole(f.id, h),
            FeatureKind::Rib(r) => self.rib(f.id, r),
            FeatureKind::Sweep(s) => self.sweep(f.id, s),
            FeatureKind::Coil(c) => self.coil(f.id, c),
            FeatureKind::Loft(l) => self.loft(f.id, l),
            FeatureKind::Draft(d) => self.draft(f.id, d),
            FeatureKind::Split(s) => self.split(f.id, s),
            FeatureKind::Combine(c) => self.combine_bodies(c),
            FeatureKind::Thread(t) => self.thread(f.id, t),
            FeatureKind::PatternRect(p) => {
                p.check()?;
                let sign = |r: bool| if r { -1.0 } else { 1.0 };
                let d1 = self.direction(&p.dir1)? * sign(p.reverse1);
                let d2 = match &p.dir2 {
                    Some(d) => self.direction(d)? * sign(p.reverse2),
                    None => Vec3::ZERO,
                };
                let mut moves = Vec::new();
                for j in 0..p.count2 {
                    for i in 0..p.count1 {
                        if i > 0 || j > 0 {
                            moves.push(Transform::Translate(d1 * (p.spacing1 * f64::from(i)) + d2 * (p.spacing2 * f64::from(j))));
                        }
                    }
                }
                self.copy_features(f.id, &p.features, &moves)
            }
            FeatureKind::PatternCircular(p) => {
                p.check()?;
                let axis = self.axis_of(&p.axis)?;
                let moves: Vec<Transform> = (1..p.count).map(|i| Transform::Rotate { axis, angle: p.step() * f64::from(i) }).collect();
                self.copy_features(f.id, &p.features, &moves)
            }
            FeatureKind::Mirror(m) => {
                let plane = self.plane_frame(&m.plane)?;
                self.copy_features(f.id, &m.features, &[Transform::Mirror { plane }])
            }
            FeatureKind::WorkPlane(w) => {
                let g = WorkGeom::Plane(self.work_plane(w)?);
                self.regen.work.insert(f.id, g);
                Ok(())
            }
            FeatureKind::WorkAxis(w) => {
                let g = WorkGeom::Axis(match w {
                    WorkAxis::Along { axis } => self.axis_of(axis)?,
                    WorkAxis::Planes { a, b } => {
                        let (a, b) = (self.plane_frame(a)?, self.plane_frame(b)?);
                        let (n1, n2) = (a.z(), b.z());
                        let dir = n1.cross(n2);
                        if dir.len() < 1e-9 {
                            return Err("the planes are parallel: they do not meet in a line".into());
                        }
                        // The point on both planes nearest the origin side of the line.
                        let (d1, d2, c) = (n1.dot(a.origin()), n2.dot(b.origin()), n1.dot(n2));
                        let det = 1.0 - c * c;
                        let p = n1 * ((d1 - d2 * c) / det) + n2 * ((d2 - d1 * c) / det);
                        Axis::new(p, dir.normalized()).ok_or("the planes do not meet in a line")?
                    }
                });
                self.regen.work.insert(f.id, g);
                Ok(())
            }
            FeatureKind::WorkPoint(w) => {
                let p = match w {
                    WorkPoint::Center { edge } => match self.edge_geometry(edge)?.curve {
                        CurveKind::Circle { axis, .. } => axis.origin(),
                        _ => return Err("the edge is not circular".into()),
                    },
                    WorkPoint::Intersection { axis, plane } => {
                        let (a, pl) = (self.axis_of(axis)?, self.plane_frame(plane)?);
                        let along = a.dir().dot(pl.z());
                        if along.abs() < 1e-9 {
                            return Err("the axis is parallel to the plane".into());
                        }
                        a.origin() + a.dir() * ((pl.origin() - a.origin()).dot(pl.z()) / along)
                    }
                };
                self.regen.work.insert(f.id, WorkGeom::Point(p));
                Ok(())
            }
        }
    }

    fn work_plane(&mut self, w: &WorkPlane) -> Result<Frame, Stop> {
        match w {
            WorkPlane::Offset { base, distance } => {
                if !distance.is_finite() {
                    return Err("the offset is not a number".into());
                }
                let f = self.plane_frame(base)?;
                Ok(Frame::new(f.origin() + f.z() * *distance, f.z(), f.x()).ok_or("the plane is not valid")?)
            }
            WorkPlane::Angle { base, axis, angle } => {
                let (f, a) = (self.plane_frame(base)?, self.axis_of(axis)?);
                let tol = 1e-6 * (1.0 + a.origin().len());
                if a.dir().dot(f.z()).abs() > 1e-6 || (a.origin() - f.origin()).dot(f.z()).abs() > tol {
                    return Err("the axis must lie in the base plane".into());
                }
                Ok(Frame::new(a.origin(), rotate(f.z(), a.dir(), *angle), a.dir()).ok_or("the plane is not valid")?)
            }
            WorkPlane::Midplane { a, b } => {
                let (a, b) = (self.plane_frame(a)?, self.plane_frame(b)?);
                if a.z().cross(b.z()).len() > 1e-6 {
                    return Err("the planes are not parallel".into());
                }
                let gap = (b.origin() - a.origin()).dot(a.z());
                Ok(Frame::new(a.origin() + a.z() * (gap / 2.0), a.z(), a.x()).ok_or("the plane is not valid")?)
            }
        }
    }

    /// The geometry of a work feature, or why there is none.
    fn work(&self, id: FeatureId) -> Result<WorkGeom, Stop> {
        self.regen.work.get(&id).copied().ok_or_else(|| format!("{} is suppressed, failed or not a work feature", label(self.doc, id)).into())
    }

    fn work_axis(&self, id: FeatureId) -> Result<Axis, Stop> {
        match self.work(id)? {
            WorkGeom::Axis(a) => Ok(a),
            _ => Err(format!("{} is not a work axis", label(self.doc, id)).into()),
        }
    }

    /// Each line is thickened symmetrically about the sketch plane and grown in the plane on one
    /// side (a slab). "To next" cuts the slab by the part and keeps the piece at the line, which
    /// ends where the part begins; it grows on the side where it meets the part (the other side
    /// when flipped, if it meets the part there too). Then the pieces join the part.
    fn rib(&mut self, id: FeatureId, r: &Rib) -> Result<(), Stop> {
        r.check()?;
        let (sketch, frame) = self.sketch_and_frame(r.sketch)?;
        let bbox = self.target_bbox().ok_or("there is no part for the rib to meet")?;
        let centre = frame.to_local(bbox.center());
        let mut pieces: Vec<(ShapeHandle, Vec<Option<FaceOrigin>>)> = Vec::new();
        let mut made = || -> Result<(), Stop> {
            for line in &r.lines {
                let (a, b) = sketch.line(*line).ok_or_else(|| format!("line {} is not a line of {}", line.0, label(self.doc, r.sketch)))?;
                if (b - a).len() < tenon_geom::tol::MIN_SIZE {
                    return Err("a rib line has no length".into());
                }
                // First choice: toward the middle of the part.
                let mut n = (b - a).normalized().perp();
                if (centre.xy() - a.mid(b)).dot(n) < 0.0 {
                    n = -n;
                }
                let reach = bbox.diagonal() * 2.0 + 1.0;
                let piece = match r.extent {
                    RibExtent::Distance(d) => {
                        // Toward the side where the part is (where it would meet the part).
                        let toward = match self.rib_side(id, r, frame, *line, (a, b), n, reach, true)? {
                            Some((s, _)) => {
                                self.k.release(s);
                                n
                            }
                            None => match self.rib_side(id, r, frame, *line, (a, b), -n, reach, true)? {
                                Some((s, _)) => {
                                    self.k.release(s);
                                    -n
                                }
                                None => n,
                            },
                        };
                        self.rib_side(id, r, frame, *line, (a, b), if r.flip { -toward } else { toward }, d, false)?
                    }
                    RibExtent::ToNext => {
                        let first = self.rib_side(id, r, frame, *line, (a, b), n, reach, true)?;
                        let second = self.rib_side(id, r, frame, *line, (a, b), -n, reach, true)?;
                        let (keep, drop) = match (first, second) {
                            (Some(x), Some(y)) if r.flip => (Some(y), Some(x)),
                            (Some(x), y) => (Some(x), y),
                            (None, y) => (y, None),
                        };
                        if let Some((s, _)) = drop {
                            self.k.release(s);
                        }
                        keep
                    }
                };
                pieces.push(piece.ok_or_else(|| format!("the rib from line {} does not meet the part on either side (give it a distance)", line.0))?);
            }
            Ok(())
        };
        if let Err(e) = made() {
            for (s, _) in pieces {
                self.k.release(s);
            }
            return Err(e);
        }
        self.combine(id, pieces, Operation::Join)
    }

    /// The rib slab from one line on side `n`, `reach` long. With `to_next`, the piece of it
    /// outside the part at the line, or `None` if it never meets the part.
    #[allow(clippy::too_many_arguments)]
    fn rib_side(
        &mut self,
        id: FeatureId,
        r: &Rib,
        frame: Frame,
        line: EntityId,
        (a, b): (Vec2, Vec2),
        n: Vec2,
        reach: f64,
        to_next: bool,
    ) -> Result<Option<(ShapeHandle, Vec<Option<FaceOrigin>>)>, Stop> {
        let quad = [a, b, b + n * reach, a + n * reach];
        let tag = |k: u64| u64::from(line.0) | (k << 24);
        let curves = (0..4).map(|i| TaggedCurve2 { tag: tag(i as u64), curve: Curve2::Line { start: quad[i], end: quad[(i + 1) % 4] } }).collect();
        let profile = Profile { frame, regions: vec![Region { outer: Loop { curves }, holes: vec![] }] };
        let slab = self.k.extrude(&profile, &Extent::Symmetric(r.thickness), None)?;
        let names = match self.k.topology(slab.shape) {
            Ok(t) => names_of_sweep(&slab.history, id, t.faces),
            Err(e) => {
                self.k.release(slab.shape);
                return Err(e.into());
            }
        };
        if !to_next {
            return Ok(Some((slab.shape, names)));
        }
        let (body_shape, body_names) = match self.regen.bodies.last() {
            Some(bd) => (bd.shape, bd.names.clone()),
            None => {
                self.k.release(slab.shape);
                return Err("there is no part for the rib to meet".into());
            }
        };
        let mut temp = vec![slab.shape];
        let mut found = || -> Result<Option<(ShapeHandle, Vec<Option<FaceOrigin>>)>, Stop> {
            let cut = self.k.boolean(BoolOp::Cut, slab.shape, &[body_shape])?;
            temp.push(cut.shape);
            let topo = self.k.topology(cut.shape)?;
            let cut_names = names_of_boolean(&cut.history, &[&names, &body_names], topo.faces);
            // Tiny balls just inside the slab at the line and at its far end tell the piece that
            // starts at the line, and whether it runs all the way (never meeting the part).
            let small = (r.thickness * 0.01).min(0.01);
            let probe = self.k.make_sphere(frame.plane_point(a.mid(b) + n * small), r.thickness * 0.001)?.shape;
            temp.push(probe);
            let far = self.k.make_sphere(frame.plane_point(a.mid(b) + n * (reach - small)), r.thickness * 0.001)?.shape;
            temp.push(far);
            for i in 0..topo.solids {
                let s = self.k.solid(cut.shape, i)?;
                let at_line = self.k.min_distance(SubShape::Shape(s.shape), SubShape::Shape(probe))?.value < 1e-6;
                let bounded = self.k.min_distance(SubShape::Shape(s.shape), SubShape::Shape(far))?.value > 1e-6;
                if at_line && bounded {
                    let faces = self.k.topology(s.shape)?.faces;
                    return Ok(Some((s.shape, names_of_boolean(&s.history, &[&cut_names], faces))));
                }
                self.k.release(s.shape);
            }
            Ok(None)
        };
        let result = found();
        for t in temp {
            self.k.release(t);
        }
        result
    }
    /// A pattern direction as a unit vector.
    fn direction(&self, d: &DirectionRef) -> Result<Vec3, Stop> {
        match d {
            DirectionRef::Origin(a) => Ok(a.axis().dir()),
            DirectionRef::Edge(e) => match self.edge_geometry(e)?.curve {
                CurveKind::Line { dir, .. } => Ok(dir),
                _ => Err("the direction edge is not straight".into()),
            },
            DirectionRef::Work(id) => Ok(self.work_axis(*id)?.dir()),
        }
    }

    /// The axis a circular pattern turns about.
    fn axis_of(&self, a: &AxisSel) -> Result<Axis, Stop> {
        match a {
            AxisSel::Origin(o) => Ok(o.axis()),
            AxisSel::Edge(e) => match self.edge_geometry(e)?.curve {
                CurveKind::Line { origin, dir } => Ok(Axis::new(origin, dir).ok_or("the axis edge has no direction")?),
                CurveKind::Circle { axis, .. } => Ok(axis),
                _ => Err("the axis edge is neither straight nor circular".into()),
            },
            AxisSel::Face(fref) => {
                let (b, face) = self.regen.resolve(fref, &*self.k).map_err(|e| self.describe_ref(fref, &e))?;
                match self.k.face_info(self.body_shape(b)?.face(face))?.surface {
                    SurfaceKind::Cylinder { axis, .. } | SurfaceKind::Cone { axis, .. } => Ok(axis),
                    _ => Err("the axis face is not cylindrical or conical".into()),
                }
            }
            AxisSel::Work(id) => self.work_axis(*id),
        }
    }

    fn edge_geometry(&self, e: &EdgeRef) -> Result<tenon_kernel::EdgeInfo, Stop> {
        let (b, i) = self.resolve_edges(std::slice::from_ref(e))?;
        let edge = *i.first().ok_or("no edge")?;
        Ok(self.k.edge_info(self.body_shape(b)?.edge(edge))?)
    }

    /// Copies the solids of `sources` by each of `moves` and combines every copy with the part as
    /// its feature did. Copied faces are named after the face copied, numbered by copy.
    fn copy_features(&mut self, id: FeatureId, sources: &[FeatureId], moves: &[Transform]) -> Result<(), Stop> {
        let mut order: Vec<FeatureId> = sources.to_vec();
        order.sort_by_key(|s| self.doc.index_of(*s));
        order.dedup();
        for src in order {
            let tools = self
                .regen
                .tools
                .get(&src)
                .cloned()
                .ok_or_else(|| format!("{} has no result to copy (it is suppressed or failed)", label(self.doc, src)))?;
            // When this pattern is copied in turn, its first occurrence (the source) goes too.
            if self.copied.contains(&id) {
                for t in &tools {
                    let op = self.k.transform(t.shape, &Transform::Translate(Vec3::ZERO))?;
                    self.regen.tools.entry(id).or_default().push(Tool { operation: t.operation, shape: op.shape, names: t.names.clone() });
                }
            }
            // Consecutive solids with the same operation combine together.
            let mut groups: Vec<(Operation, Vec<&Tool>)> = Vec::new();
            for t in &tools {
                match groups.last_mut() {
                    Some((op, g)) if *op == t.operation => g.push(t),
                    _ => groups.push((t.operation, vec![t])),
                }
            }
            for (operation, group) in groups {
                let mut copies: Vec<(ShapeHandle, Vec<Option<FaceOrigin>>)> = Vec::new();
                let mut made = || -> Result<(), Stop> {
                    for (n, mv) in moves.iter().enumerate() {
                        let ordinal = u32::try_from(n + 1).map_err(|_| "too many copies")?;
                        for t in &group {
                            let op = self.k.transform(t.shape, mv)?;
                            let faces = match self.k.topology(op.shape) {
                                Ok(topo) => topo.faces,
                                Err(e) => {
                                    self.k.release(op.shape);
                                    return Err(e.into());
                                }
                            };
                            let names = names_of_boolean(&op.history, &[&t.names], faces)
                                .into_iter()
                                .map(|o| o.map(|o| FaceOrigin::From { feature: id, source: o.key(), ordinal }))
                                .collect();
                            copies.push((op.shape, names));
                        }
                    }
                    Ok(())
                };
                if let Err(e) = made() {
                    for (s, _) in copies {
                        self.k.release(s);
                    }
                    return Err(e);
                }
                self.combine(id, copies, operation)?;
            }
        }
        Ok(())
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
            PlaneRef::Work(id) => match self.work(*id)? {
                WorkGeom::Plane(f) => Ok(f),
                _ => Err(format!("{} is not a work plane", label(self.doc, *id)).into()),
            },
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
        let profile = profile(sketch, frame, &select(sketch, &all, &e.regions)?);
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
        let taper = e.taper.unwrap_or(0.0);
        if taper == 0.0 {
            return self.combine(id, vec![(op.shape, names)], e.operation);
        }
        // Tapered: the straight solid's sides are tilted about the sketch plane, where they stay
        // put. Leaning in narrows the solid as it leaves the sketch.
        e.check()?;
        let tapered = (|| -> Result<(ShapeHandle, Vec<Option<FaceOrigin>>), Stop> {
            let topo = self.k.topology(op.shape)?;
            let sides: Vec<_> =
                (0..topo.faces).filter(|i| matches!(names.get(*i as usize), Some(Some(FaceOrigin::Side { .. })))).map(|i| op.shape.face(i)).collect();
            let away = frame.z() * sign;
            let pull = if taper > 0.0 { away } else { -away };
            let drafted = self.k.draft(op.shape, &sides, pull, taper.abs(), &frame)?;
            match self.k.topology(drafted.shape) {
                Ok(new) => Ok((drafted.shape, names_of_modify(&drafted.history, &names, &topo, id, new.faces))),
                Err(err) => {
                    self.k.release(drafted.shape);
                    Err(err.into())
                }
            }
        })();
        self.k.release(op.shape);
        let (shape, names) = tapered?;
        self.combine(id, vec![(shape, names)], e.operation)
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
            AxisRef::Work(w) => self.work_axis(*w)?,
        };
        // A flat profile turned about a line square to it sweeps no solid, and a kernel may say
        // nothing about that.
        if axis.dir().dot(frame.z()).abs() > 1.0 - 1e-9 {
            return Err("The axis is square to the sketch, so turning the profile about it makes no solid. Choose an axis that lies in the sketch's plane: a line of the sketch, or an origin axis in that plane.".into());
        }
        let all = regions(sketch);
        let profile = profile(sketch, frame, &select(sketch, &all, &r.regions)?);
        let angle = match r.angle {
            RevolveAngle::Full => AngleExtent::Full,
            RevolveAngle::Angle(a) => AngleExtent::Angle(a),
            RevolveAngle::Symmetric(a) => AngleExtent::Symmetric(a),
        };
        let op = self.k.revolve(&profile, &axis, &angle)?;
        match self.k.mass_properties(op.shape, 1.0) {
            Ok(m) if m.volume.abs() > tenon_geom::tol::MIN_SIZE => {}
            Ok(_) => {
                self.k.release(op.shape);
                return Err("The revolution has no volume: the axis is probably not in the sketch's plane, or the whole profile lies on it. Choose an axis in the sketch's plane, beside the profile or through it.".into());
            }
            Err(e) => {
                self.k.release(op.shape);
                return Err(e.into());
            }
        }
        let faces = self.k.topology(op.shape)?.faces;
        let names = names_of_sweep(&op.history, id, faces);
        self.combine(id, vec![(op.shape, names)], r.operation)
    }

    fn sweep(&mut self, id: FeatureId, s: &Sweep) -> Result<(), Stop> {
        let (sketch, frame) = self.sketch_and_frame(s.sketch)?;
        let all = regions(sketch);
        let profile = profile(sketch, frame, &select(sketch, &all, &s.regions)?);
        let (path_sketch, path_frame) = self.sketch_and_frame(s.path.sketch)?;
        let path = path_of(path_sketch, &path_frame, &s.path.curves, profile_centre(&profile))?;
        // A profile whose plane holds the path's direction sweeps into a sheet with no thickness,
        // which a kernel may build without complaint.
        if let Some(t) = path.curves.first().and_then(start_direction)
            && profile.frame.z().dot(t).abs() < 1f64.to_radians().sin()
        {
            return Err("The profile lies along the path: its sketch plane holds the direction the path starts in, so the sweep would have no thickness. Draw the profile on a plane across the path, for example square to its first line.".into());
        }
        let orientation = if s.fixed { SweepOrientation::Fixed } else { SweepOrientation::Frenet };
        let op = self.k.sweep(&profile, &path, &SweepOpts { orientation, solid: true })?;
        let faces = self.k.topology(op.shape)?.faces;
        let names = names_of_sweep(&op.history, id, faces);
        self.combine(id, vec![(op.shape, names)], s.operation)
    }

    fn coil(&mut self, id: FeatureId, c: &Coil) -> Result<(), Stop> {
        let (sketch, frame) = self.sketch_and_frame(c.sketch)?;
        let axis = match &c.axis {
            AxisRef::Origin(a) => a.axis(),
            AxisRef::SketchLine(line) => {
                let (a, b) = sketch.line(*line).ok_or("the coil axis is not a line of the sketch")?;
                let (a3, b3) = (frame.plane_point(a), frame.plane_point(b));
                Axis::new(a3, b3 - a3).ok_or("the coil axis has no length")?
            }
            AxisRef::Work(w) => self.work_axis(*w)?,
        };
        let all = regions(sketch);
        let profile = profile(sketch, frame, &select(sketch, &all, &c.regions)?);
        // Turns closer together than the profile is tall run into each other, and a kernel may
        // build that without complaint: refuse it here.
        let along: Vec<f64> = profile_points(&profile).iter().map(|q| (*q - axis.origin()).dot(axis.dir())).collect();
        let height = along.iter().copied().fold(f64::NEG_INFINITY, f64::max) - along.iter().copied().fold(f64::INFINITY, f64::min);
        if c.pitch <= height + tenon_geom::tol::LINEAR {
            return Err(format!(
                "The coil's turns run into each other: the pitch ({} mm) must be more than the profile's height along the axis ({} mm). Try a larger pitch or a smaller profile.",
                crate::explain::mm(c.pitch),
                crate::explain::mm(height)
            )
            .into());
        }
        let op = self.wind(&profile, &axis, c.pitch, c.turns, c.left)?;
        let faces = self.k.topology(op.shape)?.faces;
        let names = names_of_sweep(&op.history, id, faces);
        self.combine(id, vec![(op.shape, names)], c.operation)
    }

    fn loft(&mut self, id: FeatureId, l: &Loft) -> Result<(), Stop> {
        let mut sections = Vec::new();
        for (i, s) in l.sections.iter().enumerate() {
            let (sketch, frame) = self.sketch_and_frame(*s)?;
            let all = regions(sketch);
            if all.is_empty() {
                return Err(format!("loft section {} ({}) has no closed profile", i + 1, label(self.doc, *s)).into());
            }
            sections.push(profile(sketch, frame, &select(sketch, &all, &RegionSel::Default)?));
        }
        let op = self.k.loft(&sections, &LoftOpts { solid: true, ruled: l.ruled, closed: false })?;
        let faces = self.k.topology(op.shape)?.faces;
        let names = names_of_sweep(&op.history, id, faces);
        self.combine(id, vec![(op.shape, names)], l.operation)
    }

    fn draft(&mut self, id: FeatureId, d: &Draft) -> Result<(), Stop> {
        d.check()?;
        let neutral = self.plane_frame(&d.plane)?;
        let mut bi = None;
        let mut faces = Vec::new();
        for fref in &d.faces {
            let (b, face) = self.regen.resolve(fref, &*self.k).map_err(|e| self.describe_ref(fref, &e))?;
            if bi.is_some_and(|x| x != b) {
                return Err("the faces to draft are on different bodies".into());
            }
            bi = Some(b);
            faces.push(face);
        }
        let bi = bi.ok_or("select at least one face to draft")?;
        let shape = self.body_shape(bi)?;
        let pull = if d.reverse { -neutral.z() } else { neutral.z() };
        let op = self.k.draft(shape, &faces.iter().map(|f| shape.face(*f)).collect::<Vec<_>>(), pull, d.angle, &neutral)?;
        self.replace_body(id, bi, op)
    }

    /// One piece of `body` cut by the half-space solid `tool`; None when nothing is left of it.
    fn split_piece(&mut self, kind: BoolOp, body: &Body, tool: ShapeHandle, tool_names: &[Option<FaceOrigin>]) -> Result<Option<Body>, Stop> {
        let op = match self.k.boolean(kind, body.shape, &[tool]) {
            Ok(op) => op,
            Err(e) if e.to_string().contains("empty shape") => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let checked = self.k.mass_properties(op.shape, 1.0).map(|m| m.volume).and_then(|v| Ok((v, self.k.topology(op.shape)?.faces)));
        match checked {
            Ok((volume, _)) if volume.abs() <= tenon_geom::tol::MIN_SIZE => {
                self.k.release(op.shape);
                Ok(None)
            }
            Ok((_, faces)) => {
                let names = names_of_boolean(&op.history, &[body.names.as_slice(), tool_names], faces);
                Ok(Some(Body { shape: op.shape, names, created_by: body.created_by }))
            }
            Err(e) => {
                self.k.release(op.shape);
                Err(e.into())
            }
        }
    }

    fn split(&mut self, id: FeatureId, s: &Split) -> Result<(), Stop> {
        let plane = self.plane_frame(&s.plane)?;
        let targets: Vec<usize> = match &s.body {
            Some(fref) => vec![self.regen.resolve(fref, &*self.k).map_err(|e| self.describe_ref(fref, &e))?.0],
            None => (0..self.regen.bodies.len()).collect(),
        };
        if targets.is_empty() {
            return Err("there is no body to split".into());
        }
        let mut crossed = false;
        for bi in targets {
            let body = self.regen.bodies.get(bi).cloned().ok_or("no such body")?;
            let Some(bb) = self.k.bounding_box(body.shape)? else { continue };
            let reach = corners(&bb).into_iter().map(|c| (c - plane.origin()).dot(plane.z())).fold(f64::NEG_INFINITY, f64::max);
            if reach <= tenon_geom::tol::LINEAR {
                continue;
            }
            // Everything in front of the plane that the body could reach: a square on the plane
            // round the body's middle, as deep as the body goes.
            let (mid, half) = (bb.center() - plane.origin(), (bb.max - bb.min).len() + 1.0);
            let (cx, cy) = (mid.dot(plane.x()), mid.dot(plane.y()));
            let quad =
                [Vec2::new(cx - half, cy - half), Vec2::new(cx + half, cy - half), Vec2::new(cx + half, cy + half), Vec2::new(cx - half, cy + half)];
            let curves = (0..4).map(|i| TaggedCurve2 { tag: i as u64 + 1, curve: Curve2::Line { start: quad[i], end: quad[(i + 1) % 4] } }).collect();
            let profile = Profile { frame: plane, regions: vec![Region { outer: Loop { curves }, holes: vec![] }] };
            let front = self.k.extrude(&profile, &Extent::Distance(reach + 1.0), None)?;
            // The cut face of the front piece is the split's start face, of the back piece its
            // end face: two faces in one place that later features can tell apart.
            let pieces = (|| -> Result<(Option<Body>, Option<Body>), Stop> {
                let faces = self.k.topology(front.shape)?.faces;
                let front_names: Vec<Option<FaceOrigin>> = names_of_sweep(&front.history, id, faces)
                    .into_iter()
                    .map(|n| n.filter(|o| matches!(o, FaceOrigin::Cap { end: CapEnd::Start, .. })))
                    .collect();
                let back_names: Vec<Option<FaceOrigin>> =
                    front_names.iter().map(|n| n.map(|_| FaceOrigin::Cap { feature: id, end: CapEnd::End })).collect();
                let a = self.split_piece(BoolOp::Intersect, &body, front.shape, &front_names)?;
                let b = match self.split_piece(BoolOp::Cut, &body, front.shape, &back_names) {
                    Ok(b) => b,
                    Err(e) => {
                        if let Some(a) = a {
                            self.k.release(a.shape);
                        }
                        return Err(e);
                    }
                };
                Ok((a, b))
            })();
            self.k.release(front.shape);
            let (front_piece, back_piece) = match pieces? {
                (Some(a), Some(b)) => (a, b),
                // The plane passes the body by: it stays whole.
                (a, b) => {
                    for piece in a.into_iter().chain(b) {
                        self.k.release(piece.shape);
                    }
                    continue;
                }
            };
            crossed = true;
            let (stays, other) = match s.keep {
                SplitKeep::Front => (front_piece, back_piece),
                SplitKeep::Back | SplitKeep::Both => (back_piece, front_piece),
            };
            self.k.release(body.shape);
            if let Some(slot) = self.regen.bodies.get_mut(bi) {
                *slot = stays;
            }
            if s.keep == SplitKeep::Both {
                self.regen.bodies.push(Body { created_by: id, ..other });
            } else {
                self.k.release(other.shape);
            }
        }
        if !crossed {
            return Err("The split plane does not pass through the part, so there is nothing to split: move the plane, or choose another.".into());
        }
        Ok(())
    }

    fn combine_bodies(&mut self, c: &Combine) -> Result<(), Stop> {
        c.check()?;
        let body_of = |me: &Self, fref: &FaceRef| me.regen.resolve(fref, &*me.k).map(|r| r.0).map_err(|e| me.describe_ref(fref, &e));
        let base = body_of(self, &c.base)?;
        let mut tools: Vec<usize> = Vec::new();
        for t in &c.tools {
            let b = body_of(self, t)?;
            if b == base {
                return Err("A body cannot be combined with itself: for the other bodies, pick faces of bodies other than the base.".into());
            }
            if !tools.contains(&b) {
                tools.push(b);
            }
        }
        let kind = match c.operation {
            Operation::Cut => BoolOp::Cut,
            Operation::Intersect => BoolOp::Intersect,
            Operation::Join | Operation::NewBody => BoolOp::Union,
        };
        let target = self.regen.bodies.get(base).cloned().ok_or("no such body")?;
        let others: Vec<Body> = tools.iter().filter_map(|b| self.regen.bodies.get(*b).cloned()).collect();
        let op = self.k.boolean(kind, target.shape, &others.iter().map(|b| b.shape).collect::<Vec<_>>())?;
        let checked = self.k.mass_properties(op.shape, 1.0).map(|m| m.volume).and_then(|v| Ok((v, self.k.topology(op.shape)?.faces)));
        let faces = match checked {
            Ok((volume, _)) if volume.abs() <= tenon_geom::tol::MIN_SIZE => {
                self.k.release(op.shape);
                return Err(match c.operation {
                    Operation::Intersect => "The bodies do not overlap, so nothing would be left of the base: check which bodies are picked.",
                    _ => "This removes the whole base body, so nothing would be left: check which body is the base.",
                }
                .into());
            }
            Ok((_, faces)) => faces,
            Err(e) => {
                self.k.release(op.shape);
                return Err(e.into());
            }
        };
        let inputs: Vec<&[Option<FaceOrigin>]> = std::iter::once(target.names.as_slice()).chain(others.iter().map(|b| b.names.as_slice())).collect();
        let names = names_of_boolean(&op.history, &inputs, faces);
        self.k.release(target.shape);
        if let Some(slot) = self.regen.bodies.get_mut(base) {
            slot.shape = op.shape;
            slot.names = names;
        }
        if !c.keep_tools {
            // From the back, so the indices before stay what they were.
            tools.sort_unstable_by_key(|b| std::cmp::Reverse(*b));
            for b in tools {
                let gone = self.regen.bodies.remove(b);
                self.k.release(gone.shape);
            }
        }
        Ok(())
    }

    /// `profile` wound round `axis` in `turns` turns of `pitch`: the helix runs through the middle
    /// of the profile, which must be off the axis.
    fn wind(&mut self, profile: &Profile, axis: &Axis, pitch: f64, turns: f64, left: bool) -> Result<tenon_kernel::Op, Stop> {
        let centre = profile_centre(profile);
        let foot = axis.origin() + axis.dir() * (centre - axis.origin()).dot(axis.dir());
        let radius = centre.dist(foot);
        if radius <= tenon_geom::tol::MIN_SIZE {
            return Err("the coil profile is on its axis: move it off the axis".into());
        }
        let helix_frame = Frame::new(foot, axis.dir(), centre - foot).ok_or("the coil axis has no direction")?;
        let helix = Curve3::Helix { frame: helix_frame, radius, pitch, turns, left };
        let opts = SweepOpts { orientation: SweepOrientation::Binormal(axis.dir()), solid: true };
        Ok(self.k.sweep(profile, &Path3 { curves: vec![helix] }, &opts)?)
    }

    /// True when `body` has no material in the ring between the radii `between`, from just past
    /// `at` to one `pitch` further along `dir`: a thread's groove can run out there.
    fn free_beyond(&mut self, body: ShapeHandle, at: Vec3, dir: Vec3, radial: Vec3, between: (f64, f64), pitch: f64) -> Result<bool, Stop> {
        let frame = Frame::new(at, radial.cross(dir), radial).ok_or("the thread's axis has no direction")?;
        let (lo, hi) = between;
        let quad = [Vec2::new(lo, 0.05 * pitch), Vec2::new(hi, 0.05 * pitch), Vec2::new(hi, pitch), Vec2::new(lo, pitch)];
        let curves = (0..4).map(|i| TaggedCurve2 { tag: i as u64 + 1, curve: Curve2::Line { start: quad[i], end: quad[(i + 1) % 4] } }).collect();
        let profile = Profile { frame, regions: vec![Region { outer: Loop { curves }, holes: vec![] }] };
        let axis = Axis::new(at, dir).ok_or("the thread's axis has no direction")?;
        let ring = self.k.revolve(&profile, &axis, &AngleExtent::Full)?;
        let common = self.k.boolean(BoolOp::Intersect, body, &[ring.shape]);
        self.k.release(ring.shape);
        match common {
            Ok(op) => {
                let volume = self.k.mass_properties(op.shape, 1.0).map(|m| m.volume);
                self.k.release(op.shape);
                Ok(volume?.abs() <= tenon_geom::tol::MIN_SIZE)
            }
            // Nothing in common.
            Err(e) if e.to_string().contains("empty shape") => Ok(true),
            Err(e) => Err(e.into()),
        }
    }

    fn thread(&mut self, id: FeatureId, t: &Thread) -> Result<(), Stop> {
        use crate::explain::mm;
        t.check()?;
        let (bi, face) = self.regen.resolve(&t.face, &*self.k).map_err(|e| self.describe_ref(&t.face, &e))?;
        let shape = self.body_shape(bi)?;
        let info = self.k.face_info(shape.face(face))?;
        let SurfaceKind::Cylinder { axis, radius } = info.surface else {
            return Err("A thread goes on a cylindrical face: a round shaft or a round hole. Pick one of those.".into());
        };
        // A face turned against its surface's own normal has its material outside: a hole.
        let internal = info.reversed;
        let pitch = t.pitch.unwrap_or_else(|| crate::threads::default_pitch(2.0 * radius, internal));
        let depth = crate::threads::DEPTH * pitch;
        let major = crate::threads::nominal(2.0 * radius, pitch, internal);
        // The face's ends along its axis: a whole cylinder's area is its circumference times its
        // length, about its centroid. The axis is taken the way its largest part is positive
        // (up, for an upright part), whichever way the kernel happens to hold it.
        let up = {
            let d = axis.dir();
            let largest = [d.x, d.y, d.z].into_iter().fold(0.0f64, |a, b| if b.abs() > a.abs() { b } else { a });
            if largest < 0.0 { -d } else { d }
        };
        let whole = info.area / (std::f64::consts::TAU * radius);
        let mid = (info.centroid - axis.origin()).dot(up);
        let (low, high) = (axis.origin() + up * (mid - whole / 2.0), axis.origin() + up * (mid + whole / 2.0));
        // Which ends are open: nothing of the part where the thread's groove would run on past
        // them (a bolt's tip, a hole's mouth; not a shoulder or a blind hole's bottom).
        let radial = Frame::from_normal(high, up).ok_or("the thread's axis has no direction")?.x();
        let ring = if internal { (radius - 0.05 * pitch, radius + depth) } else { (radius - depth, radius + 0.05 * pitch) };
        let open_high = self.free_beyond(shape, high, up, radial, ring, pitch)?;
        let open_low = self.free_beyond(shape, low, -up, radial, ring, pitch)?;
        // A thread starts at its open end. With both ends open, or neither, at the high one.
        let from_high = (open_high || !open_low) != t.reverse;
        let (start, dir, free_start, free_far) = if from_high { (high, -up, open_high, open_low) } else { (low, up, open_low, open_high) };
        let length = match t.length {
            ThreadLength::Full => whole,
            ThreadLength::Distance(d) if d > whole + tenon_geom::tol::LINEAR => {
                return Err(format!(
                    "The thread is {} mm long but its face is only {} mm long: make it shorter, or let it run the full length.",
                    mm(d),
                    mm(whole)
                )
                .into());
            }
            ThreadLength::Distance(d) => d,
        };
        if length < pitch {
            return Err(format!(
                "The thread is {} mm long, less than one turn of its {} mm pitch: use a finer pitch or a longer thread.",
                mm(length),
                mm(pitch)
            )
            .into());
        }
        if !internal && depth >= 0.8 * radius {
            return Err(format!(
                "A {} mm pitch is too coarse for a {} mm shaft: the thread would cut most of it away. Use a finer pitch.",
                mm(pitch),
                mm(2.0 * radius)
            )
            .into());
        }
        if t.modelled {
            // The groove between two turns, in a plane through the axis (x away from the axis,
            // y along the thread): flanks at 30 degrees from square to the axis. It reaches a
            // little past the face so the cut is clean.
            let tan30 = 1.0 / 3f64.sqrt();
            let over = pitch / 32.0 / tan30;
            let (at_face, at_root) = if internal { (0.75 * pitch, 0.125 * pitch) } else { (0.875 * pitch, 0.25 * pitch) };
            let wide = at_face + 2.0 * over * tan30;
            let (r_wide, r_root) = if internal { (radius - over, radius + depth) } else { (radius + over, radius - depth) };
            let quad =
                [Vec2::new(r_wide, -wide / 2.0), Vec2::new(r_wide, wide / 2.0), Vec2::new(r_root, at_root / 2.0), Vec2::new(r_root, -at_root / 2.0)];
            let body = self.regen.bodies.get(bi).cloned().ok_or("no such body")?;
            // Past an end that is free (a shaft's end, a hole's mouth) the groove runs on, so the
            // thread runs out cleanly: a whole turn before the start, and most of one after the
            // end. Where the face ends at a shoulder or a blind hole's bottom, the groove stops
            // at the end of the face instead.
            let free_end = t.length == ThreadLength::Full && free_far;
            // (Short of a closed end by a little: a groove that only touches the end face is a case
            // booleans get wrong.)
            let closed = -(wide / 2.0 + 0.05 * pitch);
            let lead_in = if free_start { pitch } else { closed };
            // What each turn takes out of the part: the groove's section inside the face, times
            // the way its middle goes round (Pappus).
            let section = (at_face + at_root) / 2.0 * depth;
            let off = depth * (at_face + 2.0 * at_root) / (3.0 * (at_face + at_root));
            let per_turn = section * std::f64::consts::TAU * if internal { radius + off } else { radius - off };
            let before = self.k.mass_properties(body.shape, 1.0)?.volume;
            // A kernel can return a wrong solid for a groove that passes clean through both ends
            // (OCCT returns nothing at all for some lengths, without an error). So every cut is
            // checked by the volume it took, and a wrong one is tried again with the groove
            // running out a little less far.
            let run_outs: &[f64] = if free_end { &[0.5, 0.72, 0.31] } else { &[0.0] };
            let mut cut = None;
            for out in run_outs {
                let lead_out = if free_end { out * pitch } else { closed };
                let run = length + lead_in + lead_out;
                if run < pitch / 2.0 {
                    return Err("The thread has no room between the ends of its face: use a finer pitch or a longer face.".into());
                }
                let origin = start - dir * lead_in;
                let frame = Frame::new(origin, radial.cross(dir), radial).ok_or("the thread's axis has no direction")?;
                let curves =
                    (0..4).map(|i| TaggedCurve2 { tag: i as u64 + 1, curve: Curve2::Line { start: quad[i], end: quad[(i + 1) % 4] } }).collect();
                let profile = Profile { frame, regions: vec![Region { outer: Loop { curves }, holes: vec![] }] };
                let helix_axis = Axis::new(origin, dir).ok_or("the thread's axis has no direction")?;
                let tool = self.wind(&profile, &helix_axis, pitch, run / pitch, t.left)?;
                let made = (|| -> Result<Option<(ShapeHandle, Vec<Option<FaceOrigin>>)>, Stop> {
                    let tool_names = names_of_sweep(&tool.history, id, self.k.topology(tool.shape)?.faces);
                    let op = self.k.boolean(BoolOp::Cut, body.shape, &[tool.shape])?;
                    let checked = self.k.mass_properties(op.shape, 1.0).map(|m| m.volume).and_then(|v| Ok((v, self.k.topology(op.shape)?.faces)));
                    match checked {
                        Ok((after, faces)) => {
                            // Within a turn and a half of the turns asked for.
                            let turns = (before - after) / per_turn;
                            if after > 0.0 && (turns - length / pitch).abs() <= 1.5 {
                                Ok(Some((op.shape, names_of_boolean(&op.history, &[body.names.as_slice(), tool_names.as_slice()], faces))))
                            } else {
                                self.k.release(op.shape);
                                Ok(None)
                            }
                        }
                        Err(e) => {
                            self.k.release(op.shape);
                            Err(e.into())
                        }
                    }
                })();
                self.k.release(tool.shape);
                cut = made?;
                if cut.is_some() {
                    break;
                }
            }
            let Some((cut_shape, names)) = cut else {
                return Err("The thread could not be cut into the part: the geometry kernel gave a wrong solid for it. Try a slightly different length or pitch, or leave the thread cosmetic.".into());
            };
            self.k.release(body.shape);
            if let Some(slot) = self.regen.bodies.get_mut(bi) {
                slot.shape = cut_shape;
                slot.names = names;
            }
        }
        self.regen.threads.push(ThreadMark {
            feature: id,
            face: t.face.clone(),
            designation: t.designation.clone().unwrap_or_else(|| crate::threads::designation(major, pitch)),
            pitch,
            diameter: major,
            internal,
            length,
            start,
            direction: dir,
            left: t.left,
            modelled: t.modelled,
        });
        Ok(())
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
        // A pattern or mirror copies this feature later: keep its solids.
        let keep = self.copied.contains(&id);
        let Some(kind) = kind else {
            if keep {
                // The solids become bodies (and change); the pattern gets copies of them.
                let mut kept = Vec::new();
                for (shape, names) in &tools {
                    match self.k.transform(*shape, &Transform::Translate(Vec3::ZERO)) {
                        Ok(op) => kept.push(Tool { operation, shape: op.shape, names: names.clone() }),
                        Err(e) => {
                            for s in kept.iter().map(|t| t.shape).chain(tools.iter().map(|t| t.0)) {
                                self.k.release(s);
                            }
                            return Err(e.into());
                        }
                    }
                }
                self.regen.tools.entry(id).or_default().extend(kept);
            }
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
        if keep && op.is_ok() {
            self.regen.tools.entry(id).or_default().extend(tools.iter().map(|(shape, names)| Tool {
                operation,
                shape: *shape,
                names: names.clone(),
            }));
        } else {
            for s in shapes {
                self.k.release(s);
            }
        }
        let op = op?;
        let volume = self.k.mass_properties(op.shape, 1.0)?.volume;
        if volume.abs() <= tenon_geom::tol::MIN_SIZE {
            self.k.release(op.shape);
            return Err(match operation {
                Operation::Intersect => "The feature and the part do not overlap, so nothing of the part would be left: check its size and position.",
                _ => "This removes the whole part, so nothing would be left: try a smaller size, a shorter distance, or the other direction.",
            }
            .into());
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
    /// Per edge: its curve (lines and circles carry their geometry).
    pub curves: Vec<CurveKind>,
    /// Per edge: its start and end points (the same point for a closed edge such as a circle).
    pub ends: Vec<[Vec3; 2]>,
}

impl BodyView {
    /// A persistent reference to edge `edge`, if both its faces are named.
    pub fn edge_ref(&self, edge: u32) -> Option<EdgeRef> {
        let (names, fp) = self.edges.get(edge as usize)?;
        let [a, b] = (*names)?;
        Some(EdgeRef::new(a, b, fp.clone()))
    }
}

/// A thread and where its face is among the bodies shown: (body, face), when the face is still
/// there.
#[derive(Clone, Debug, PartialEq)]
pub struct ThreadView {
    pub mark: ThreadMark,
    pub at: Option<(usize, u32)>,
}

/// Everything the UI needs to draw a regeneration result (no kernel handles).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene {
    pub bodies: Vec<BodyView>,
    pub status: Vec<(FeatureId, FeatureStatus)>,
    pub sketch_frames: BTreeMap<FeatureId, Frame>,
    /// Work planes, axes and points, to draw.
    pub work: Vec<(FeatureId, WorkGeom)>,
    /// The threads on the part.
    pub threads: Vec<ThreadView>,
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
        let mut curves = Vec::with_capacity(topo.edges as usize);
        let mut ends = Vec::with_capacity(topo.edges as usize);
        for (i, adj) in topo.edge_faces.iter().enumerate() {
            let info = k.edge_info(b.shape.edge(u32::try_from(i).map_err(|_| "too many edges")?)).map_err(kerr)?;
            edges.push((edge_names(&b.names, adj), EdgeFingerprint::of(&info)));
            ends.push([info.start, info.end]);
            curves.push(info.curve);
        }
        let mass = k.mass_properties(b.shape, 1.0).map_err(kerr)?;
        bodies.push(BodyView { mesh, faces, edges, curves, ends, volume: mass.volume, mass, bbox: k.bounding_box(b.shape).map_err(kerr)? });
    }
    Ok(Scene {
        bodies,
        status: regen.status.clone(),
        sketch_frames: regen.sketch_frames.clone(),
        work: regen.work.iter().map(|(id, g)| (*id, *g)).collect(),
        threads: regen.threads.iter().map(|m| ThreadView { at: regen.resolve(&m.face, &*k).ok(), mark: m.clone() }).collect(),
        regen_ms: regen.millis,
        mesh_ms: t0.elapsed().as_secs_f64() * 1000.0,
    })
}

/// Points along a profile's outer boundaries, in space (arcs and circles sampled; splines by
/// their poles, which bound them).
fn profile_points(p: &Profile) -> Vec<Vec3> {
    let mut pts = Vec::new();
    let round = |pts: &mut Vec<Vec2>, center: Vec2, radius: f64, a0: f64, sweep: f64| {
        pts.extend((0..16).map(|i| {
            let a = a0 + sweep * f64::from(i) / 16.0;
            center + Vec2::new(a.cos(), a.sin()) * radius
        }));
    };
    for r in &p.regions {
        for c in &r.outer.curves {
            match &c.curve {
                Curve2::Line { start, .. } => pts.push(*start),
                Curve2::Arc { center, radius, start_angle, end_angle } => {
                    let mut sweep = end_angle - start_angle;
                    while sweep <= 0.0 {
                        sweep += std::f64::consts::TAU;
                    }
                    round(&mut pts, *center, *radius, *start_angle, sweep);
                }
                Curve2::Circle { center, radius } => round(&mut pts, *center, *radius, 0.0, std::f64::consts::TAU),
                Curve2::BSpline { poles, .. } => pts.extend(poles.iter().copied()),
            }
        }
    }
    pts.into_iter().map(|q| p.frame.plane_point(q)).collect()
}

/// About the middle of a profile: the average of points along its outer boundaries.
fn profile_centre(p: &Profile) -> Vec3 {
    let pts = profile_points(p);
    let n = pts.len().max(1) as f64;
    pts.into_iter().fold(Vec3::ZERO, |a, b| a + b) * (1.0 / n)
}

/// A sweep path: the sketch's lines and arcs `curves`, joined end to end and placed in space,
/// starting at whichever end of the chain is nearer `near` (the profile).
/// The unit direction a path curve sets off in.
fn start_direction(c: &Curve3) -> Option<Vec3> {
    match c {
        Curve3::Line { start, end } => Some((*end - *start).normalized()).filter(|d| d.len() > 0.0),
        Curve3::Arc { start, mid, end } => {
            // Square to the radius at the start, in the arc's plane, heading towards the middle.
            let (a, b) = (*mid - *start, *end - *start);
            let n = a.cross(b);
            let centre = *start + (b.cross(n) * a.dot(a) + n.cross(a) * b.dot(b)) * (0.5 / n.dot(n));
            let t = Some(n.cross(*start - centre).normalized()).filter(|d| d.len() > 0.0)?;
            Some(if t.dot(a) < 0.0 { -t } else { t })
        }
        Curve3::Helix { .. } => None,
    }
}

fn path_of(sketch: &Sketch, frame: &Frame, curves: &[EntityId], near: Vec3) -> Result<Path3, String> {
    // Each curve by its end point ids: lines and arcs share points where they join.
    let mut segs = Vec::new();
    for id in curves {
        match sketch.geometry(*id) {
            Some(tenon_sketch::Geometry::Line { start, end } | tenon_sketch::Geometry::Arc { start, end, .. }) => segs.push((*id, *start, *end)),
            Some(_) => return Err(format!("a sweep path is lines and arcs only, and e{} is neither", id.0)),
            None => return Err(format!("the sweep path's e{} is not in its sketch", id.0)),
        }
    }
    let ends = |p: EntityId| segs.iter().filter(|s| s.1 == p || s.2 == p).count();
    if segs.is_empty() {
        return Err("the sweep path is empty".into());
    }
    // Start from a free end (an open path), or anywhere on a closed one.
    let first = segs.iter().position(|s| ends(s.1) == 1 || ends(s.2) == 1).unwrap_or(0);
    let mut at = if ends(segs[first].2) == 1 && ends(segs[first].1) != 1 { segs[first].2 } else { segs[first].1 };
    let mut left = segs.clone();
    let mut chain = Vec::new();
    while !left.is_empty() {
        let Some(i) = left.iter().position(|s| s.1 == at || s.2 == at) else {
            return Err("the sweep path's curves do not join end to end".into());
        };
        let (id, a, b) = left.remove(i);
        let forward = a == at;
        at = if forward { b } else { a };
        chain.push((id, forward));
    }
    let mut out = Vec::new();
    for (id, forward) in &chain {
        if let Some((a, b)) = sketch.line(*id) {
            let (a, b) = if *forward { (a, b) } else { (b, a) };
            out.push(Curve3::Line { start: frame.plane_point(a), end: frame.plane_point(b) });
        } else if let Some(arc) = sketch.arc(*id) {
            let at_angle = |t: f64| frame.plane_point(arc.center + Vec2::new(t.cos(), t.sin()) * arc.radius);
            let (s, m, e) = (at_angle(arc.start), at_angle(arc.start + arc.sweep() / 2.0), at_angle(arc.start + arc.sweep()));
            let (s, e) = if *forward { (s, e) } else { (e, s) };
            out.push(Curve3::Arc { start: s, mid: m, end: e });
        }
    }
    // Begin at the end nearer the profile.
    let start_of = |c: &Curve3| match c {
        Curve3::Line { start, .. } | Curve3::Arc { start, .. } => *start,
        Curve3::Helix { frame, .. } => frame.origin(),
    };
    let end_of = |c: &Curve3| match c {
        Curve3::Line { end, .. } | Curve3::Arc { end, .. } => *end,
        Curve3::Helix { frame, .. } => frame.origin(),
    };
    let (Some(head), Some(tail)) = (out.first(), out.last()) else { return Err("the sweep path is empty".into()) };
    if end_of(tail).dist(near) < start_of(head).dist(near) {
        out.reverse();
        for c in &mut out {
            let turned = match &*c {
                Curve3::Line { start, end } => Curve3::Line { start: *end, end: *start },
                Curve3::Arc { start, mid, end } => Curve3::Arc { start: *end, mid: *mid, end: *start },
                h => h.clone(),
            };
            *c = turned;
        }
    }
    Ok(Path3 { curves: out })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_sets_off_along_its_first_line_or_square_to_its_first_arcs_radius() {
        let line = Curve3::Line { start: Vec3::ZERO, end: Vec3::new(0.0, 0.0, 5.0) };
        assert_eq!(start_direction(&line), Some(Vec3::Z));
        assert_eq!(start_direction(&Curve3::Line { start: Vec3::X, end: Vec3::X }), None);
        // A quarter circle round (10, 0, 0) from the origin, up and over: it sets off along +Z.
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let arc = Curve3::Arc { start: Vec3::ZERO, mid: Vec3::new(10.0 - 10.0 * s, 0.0, 10.0 * s), end: Vec3::new(10.0, 0.0, 10.0) };
        let t = start_direction(&arc).unwrap();
        assert!((t - Vec3::Z).len() < 1e-12, "{t:?}");
        // Run backwards it sets off along -X from its far end.
        let back = Curve3::Arc { start: Vec3::new(10.0, 0.0, 10.0), mid: Vec3::new(10.0 - 10.0 * s, 0.0, 10.0 * s), end: Vec3::ZERO };
        let t = start_direction(&back).unwrap();
        assert!((t + Vec3::X).len() < 1e-12, "{t:?}");
    }
}
