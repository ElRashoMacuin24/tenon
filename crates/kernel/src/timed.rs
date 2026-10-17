//! A kernel that keeps count of what is asked of the kernel inside it, and how long each kind
//! of call takes in all: for benchmarks, to say where the time of a rebuild goes.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Instant;

use tenon_geom::{Aabb3, Axis, Frame, Vec3};

use crate::{
    AngleExtent, BoolOp, CancelToken, ChamferSpec, Distance, EdgeId, EdgeInfo, Extent, FaceId, FaceInfo, HlrCurve, HoleSpec, KResult, Kernel,
    LoftOpts, MassProps, Mesh, MeshTol, Op, Path3, Pattern, Profile, ShapeHandle, SubShape, SweepOpts, Topology, Transform,
};

/// What was asked of a kernel: for each kind of call, how often and how many milliseconds.
pub type Tally = BTreeMap<&'static str, (u64, f64)>;

/// A kernel wrapped to keep a [`Tally`] of its calls.
pub struct Timed {
    inner: Box<dyn Kernel>,
    tally: Mutex<Tally>,
}

impl Timed {
    pub fn new(inner: Box<dyn Kernel>) -> Timed {
        Timed { inner, tally: Mutex::new(Tally::new()) }
    }

    /// The tally so far.
    pub fn tally(&self) -> Tally {
        self.tally.lock().map(|t| t.clone()).unwrap_or_default()
    }

    /// Starts the tally again from nothing.
    pub fn reset(&self) {
        if let Ok(mut t) = self.tally.lock() {
            t.clear();
        }
    }

    fn note(&self, what: &'static str, since: Instant) {
        if let Ok(mut t) = self.tally.lock() {
            let entry = t.entry(what).or_insert((0, 0.0));
            entry.0 += 1;
            entry.1 += since.elapsed().as_secs_f64() * 1000.0;
        }
    }
}

/// One call passed on to the kernel inside, and noted.
macro_rules! timed {
    ($self:ident, $what:literal, $call:expr) => {{
        let since = Instant::now();
        let out = $call;
        $self.note($what, since);
        out
    }};
}

impl Kernel for Timed {
    fn name(&self) -> &'static str {
        self.inner.name()
    }
    fn version(&self) -> String {
        self.inner.version()
    }
    fn make_box(&mut self, frame: &Frame, size: Vec3) -> KResult<Op> {
        timed!(self, "make_box", self.inner.make_box(frame, size))
    }
    fn make_cylinder(&mut self, axis: &Axis, radius: f64, height: f64) -> KResult<Op> {
        timed!(self, "make_cylinder", self.inner.make_cylinder(axis, radius, height))
    }
    fn make_cone(&mut self, axis: &Axis, r1: f64, r2: f64, height: f64) -> KResult<Op> {
        timed!(self, "make_cone", self.inner.make_cone(axis, r1, r2, height))
    }
    fn make_sphere(&mut self, center: Vec3, radius: f64) -> KResult<Op> {
        timed!(self, "make_sphere", self.inner.make_sphere(center, radius))
    }
    fn make_torus(&mut self, axis: &Axis, major_radius: f64, minor_radius: f64) -> KResult<Op> {
        timed!(self, "make_torus", self.inner.make_torus(axis, major_radius, minor_radius))
    }
    fn make_face(&mut self, profile: &Profile) -> KResult<Op> {
        timed!(self, "make_face", self.inner.make_face(profile))
    }
    fn extrude(&mut self, profile: &Profile, extent: &Extent, taper: Option<f64>) -> KResult<Op> {
        timed!(self, "extrude", self.inner.extrude(profile, extent, taper))
    }
    fn revolve(&mut self, profile: &Profile, axis: &Axis, angle: &AngleExtent) -> KResult<Op> {
        timed!(self, "revolve", self.inner.revolve(profile, axis, angle))
    }
    fn roll(&mut self, profile: &Profile, rho: f64, inner: f64, outer: f64) -> KResult<Op> {
        timed!(self, "roll", self.inner.roll(profile, rho, inner, outer))
    }
    fn sweep(&mut self, profile: &Profile, path: &Path3, opts: &SweepOpts) -> KResult<Op> {
        timed!(self, "sweep", self.inner.sweep(profile, path, opts))
    }
    fn loft(&mut self, sections: &[Profile], opts: &LoftOpts) -> KResult<Op> {
        timed!(self, "loft", self.inner.loft(sections, opts))
    }
    fn draft(&mut self, body: ShapeHandle, faces: &[FaceId], pull: Vec3, angle: f64, neutral: &Frame) -> KResult<Op> {
        timed!(self, "draft", self.inner.draft(body, faces, pull, angle, neutral))
    }
    fn fillet(&mut self, body: ShapeHandle, edges: &[EdgeId], radius: f64) -> KResult<Op> {
        timed!(self, "fillet", self.inner.fillet(body, edges, radius))
    }
    fn fillet_variable(&mut self, body: ShapeHandle, edges: &[EdgeId], law: &[(f64, f64)]) -> KResult<Op> {
        timed!(self, "fillet", self.inner.fillet_variable(body, edges, law))
    }
    fn chamfer(&mut self, body: ShapeHandle, edges: &[EdgeId], spec: &ChamferSpec) -> KResult<Op> {
        timed!(self, "chamfer", self.inner.chamfer(body, edges, spec))
    }
    fn unify(&mut self, body: ShapeHandle, faces: &[FaceId]) -> KResult<Op> {
        timed!(self, "unify", self.inner.unify(body, faces))
    }
    fn shell(&mut self, body: ShapeHandle, open_faces: &[FaceId], thickness: f64) -> KResult<Op> {
        timed!(self, "shell", self.inner.shell(body, open_faces, thickness))
    }
    fn hole(&mut self, body: ShapeHandle, spec: &HoleSpec) -> KResult<Op> {
        timed!(self, "hole", self.inner.hole(body, spec))
    }
    fn boolean(&mut self, op: BoolOp, target: ShapeHandle, tools: &[ShapeHandle]) -> KResult<Op> {
        timed!(self, "boolean", self.inner.boolean(op, target, tools))
    }
    fn transform(&mut self, shape: ShapeHandle, transform: &Transform) -> KResult<Op> {
        timed!(self, "transform", self.inner.transform(shape, transform))
    }
    fn pattern(&mut self, shape: ShapeHandle, pattern: &Pattern) -> KResult<Op> {
        timed!(self, "pattern", self.inner.pattern(shape, pattern))
    }
    fn topology(&self, shape: ShapeHandle) -> KResult<Topology> {
        timed!(self, "topology", self.inner.topology(shape))
    }
    fn face_info(&self, face: FaceId) -> KResult<FaceInfo> {
        timed!(self, "face_info", self.inner.face_info(face))
    }
    fn edge_info(&self, edge: EdgeId) -> KResult<EdgeInfo> {
        timed!(self, "edge_info", self.inner.edge_info(edge))
    }
    fn edge_points(&self, edge: EdgeId, deflection: f64) -> KResult<Vec<Vec3>> {
        timed!(self, "edge_points", self.inner.edge_points(edge, deflection))
    }
    fn tessellate(&mut self, shape: ShapeHandle, tol: &MeshTol) -> KResult<Mesh> {
        timed!(self, "tessellate", self.inner.tessellate(shape, tol))
    }
    fn mass_properties(&self, shape: ShapeHandle, density: f64) -> KResult<MassProps> {
        timed!(self, "mass_properties", self.inner.mass_properties(shape, density))
    }
    fn bounding_box(&self, shape: ShapeHandle) -> KResult<Option<Aabb3>> {
        timed!(self, "bounding_box", self.inner.bounding_box(shape))
    }
    fn is_valid(&self, shape: ShapeHandle) -> KResult<bool> {
        timed!(self, "is_valid", self.inner.is_valid(shape))
    }
    fn import_step(&mut self, data: &[u8]) -> KResult<Vec<ShapeHandle>> {
        timed!(self, "import_step", self.inner.import_step(data))
    }
    fn export_step(&self, shapes: &[ShapeHandle]) -> KResult<Vec<u8>> {
        timed!(self, "export_step", self.inner.export_step(shapes))
    }
    fn set_cancel(&mut self, token: CancelToken) {
        self.inner.set_cancel(token);
    }
    fn release(&mut self, shape: ShapeHandle) {
        timed!(self, "release", self.inner.release(shape));
    }
    fn solid(&mut self, shape: ShapeHandle, index: u32) -> KResult<Op> {
        timed!(self, "solid", self.inner.solid(shape, index))
    }
    fn min_distance(&self, a: SubShape, b: SubShape) -> KResult<Distance> {
        timed!(self, "min_distance", self.inner.min_distance(a, b))
    }
    fn duplicate(&mut self, shape: ShapeHandle) -> KResult<ShapeHandle> {
        timed!(self, "duplicate", self.inner.duplicate(shape))
    }
    fn project_edges(&mut self, shapes: &[ShapeHandle], view: &Frame, deflection: f64, hidden: bool) -> KResult<Vec<HlrCurve>> {
        timed!(self, "project_edges", self.inner.project_edges(shapes, view, deflection, hidden))
    }
    fn live_shapes(&self) -> usize {
        self.inner.live_shapes()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::NullKernel;

    #[test]
    fn a_timed_kernel_passes_calls_on_and_counts_them() {
        let mut k = Timed::new(Box::new(NullKernel));
        assert_eq!(k.name(), "null");
        assert!(k.make_sphere(Vec3::ZERO, 1.0).is_err(), "the kernel inside refuses, and its answer comes back");
        let _ = k.make_sphere(Vec3::ZERO, 2.0);
        let _ = k.mass_properties(ShapeHandle::from_parts(0, 0), 1.0);
        let tally = k.tally();
        assert_eq!(tally.keys().copied().collect::<Vec<_>>(), ["make_sphere", "mass_properties"]);
        assert_eq!((tally["make_sphere"].0, tally["mass_properties"].0), (2, 1));
        assert!(tally.values().all(|(_, ms)| *ms >= 0.0));
        k.reset();
        assert!(k.tally().is_empty());
    }
}
