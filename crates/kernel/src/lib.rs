//! Tenon kernel: the backend-neutral [`Kernel`] trait and its types.
//!
//! Every geometry operation in Tenon goes through this trait, so the OpenCASCADE backend
//! (`tenon-kernel-occt`) can later be replaced, operation by operation, by a Rust-native kernel.
//! Nothing in this crate knows about any backend.
//!
//! Contract for backends:
//! - Shapes live inside the kernel and are addressed by generational [`ShapeHandle`]s.
//! - Every modelling operation returns an [`Op`] whose [`History`] says where each face and edge
//!   of the result came from. This feeds persistent naming (docs/persistent-naming.md).
//! - Errors are typed [`KernelError`]s. A backend never panics and never lets a native exception
//!   cross the trait boundary. Inputs are validated with [`check`] before native code runs.
//! - Units are millimetres and radians. Tolerances come from [`tenon_geom::tol`].
//! - Operations a backend does not support yet return [`KernelError::Unsupported`] (the default
//!   method bodies), so the trait can be complete before every backend is.
#![forbid(unsafe_code)]

pub mod check;
mod handle;
mod history;
mod query;
mod types;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub use handle::{EdgeId, FaceId, ShapeHandle, TopoId, TopoKind, VertexId};
pub use history::{Generated, History, Image, InputRef, Op, Origin, PrimitiveRole};
pub use query::{CurveKind, EdgeInfo, EdgePolyline, FaceInfo, FaceRange, MassProps, Mesh, MeshTol, ShapeKind, SurfaceKind, Topology};
use tenon_geom::{Aabb3, Axis, Frame, Vec3};
pub use types::{
    AngleExtent, BoolOp, ChamferSpec, Curve2, Curve3, Extent, HoleDepth, HoleKind, HoleSpec, LoftOpts, Loop, Path3, Pattern, Profile, Region,
    SweepOpts, SweepOrientation, TaggedCurve2, Transform,
};

/// Result type of every kernel call.
pub type KResult<T> = Result<T, KernelError>;

/// Why a kernel call failed.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum KernelError {
    /// The handle was released or never belonged to this kernel.
    #[error("invalid or released shape handle")]
    InvalidHandle,
    /// Rejected before any geometry was attempted.
    #[error("invalid input: {0}")]
    InvalidInput(String),
    /// The geometric operation itself failed (for example a fillet radius too large).
    #[error("{op} failed: {reason}")]
    OperationFailed { op: &'static str, reason: String },
    /// This backend does not implement the operation (yet).
    #[error("{0} is not supported by this kernel backend yet")]
    Unsupported(&'static str),
    /// Reading or writing an exchange format failed.
    #[error("data exchange failed: {0}")]
    Exchange(String),
    /// Stopped through the [`CancelToken`].
    #[error("cancelled")]
    Cancelled,
    /// An unexpected error inside the backend.
    #[error("kernel backend error: {0}")]
    Backend(String),
}

/// Cooperative cancellation for long operations. Clones share the same flag.
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn reset(&self) {
        self.0.store(false, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

macro_rules! unsupported {
    ($op:literal) => {
        Err(KernelError::Unsupported($op))
    };
}

/// A B-rep modelling kernel.
///
/// Object safe: the application holds a `Box<dyn Kernel>`. `Send` so a kernel can live on the
/// regeneration worker thread; a kernel is used from one thread at a time.
pub trait Kernel: Send {
    /// Backend name, for diagnostics ("occt", "null", ...).
    fn name(&self) -> &'static str;
    /// Backend version, for diagnostics.
    fn version(&self) -> String;

    // ---- primitives -------------------------------------------------------------------------

    /// Box with one corner at the frame origin, extending `size` along the frame's X, Y, Z.
    fn make_box(&mut self, frame: &Frame, size: Vec3) -> KResult<Op> {
        let _ = (frame, size);
        unsupported!("make_box")
    }
    /// Cylinder on `axis`, from its origin, `height` along its direction.
    fn make_cylinder(&mut self, axis: &Axis, radius: f64, height: f64) -> KResult<Op> {
        let _ = (axis, radius, height);
        unsupported!("make_cylinder")
    }
    /// Cone (or frustum) on `axis`: radius `r1` at the origin, `r2` at `height`.
    fn make_cone(&mut self, axis: &Axis, r1: f64, r2: f64, height: f64) -> KResult<Op> {
        let _ = (axis, r1, r2, height);
        unsupported!("make_cone")
    }
    fn make_sphere(&mut self, center: Vec3, radius: f64) -> KResult<Op> {
        let _ = (center, radius);
        unsupported!("make_sphere")
    }
    fn make_torus(&mut self, axis: &Axis, major_radius: f64, minor_radius: f64) -> KResult<Op> {
        let _ = (axis, major_radius, minor_radius);
        unsupported!("make_torus")
    }
    /// Planar face(s) from a profile.
    fn make_face(&mut self, profile: &Profile) -> KResult<Op> {
        let _ = profile;
        unsupported!("make_face")
    }

    // ---- features ---------------------------------------------------------------------------

    /// Solid swept straight from a profile along its frame's Z; optional taper (radians).
    fn extrude(&mut self, profile: &Profile, extent: &Extent, taper: Option<f64>) -> KResult<Op> {
        let _ = (profile, extent, taper);
        unsupported!("extrude")
    }
    fn revolve(&mut self, profile: &Profile, axis: &Axis, angle: &AngleExtent) -> KResult<Op> {
        let _ = (profile, axis, angle);
        unsupported!("revolve")
    }
    fn sweep(&mut self, profile: &Profile, path: &Path3, opts: &SweepOpts) -> KResult<Op> {
        let _ = (profile, path, opts);
        unsupported!("sweep")
    }
    fn loft(&mut self, sections: &[Profile], opts: &LoftOpts) -> KResult<Op> {
        let _ = (sections, opts);
        unsupported!("loft")
    }
    /// Constant-radius fillet on edges of `body`.
    fn fillet(&mut self, body: ShapeHandle, edges: &[EdgeId], radius: f64) -> KResult<Op> {
        let _ = (body, edges, radius);
        unsupported!("fillet")
    }
    fn chamfer(&mut self, body: ShapeHandle, edges: &[EdgeId], spec: &ChamferSpec) -> KResult<Op> {
        let _ = (body, edges, spec);
        unsupported!("chamfer")
    }
    /// Hollow out `body`, removing `open_faces`; positive `thickness` keeps the outer skin.
    fn shell(&mut self, body: ShapeHandle, open_faces: &[FaceId], thickness: f64) -> KResult<Op> {
        let _ = (body, open_faces, thickness);
        unsupported!("shell")
    }
    fn hole(&mut self, body: ShapeHandle, spec: &HoleSpec) -> KResult<Op> {
        let _ = (body, spec);
        unsupported!("hole")
    }
    /// `target` combined with `tools`. History input 0 is the target, 1.. the tools.
    fn boolean(&mut self, op: BoolOp, target: ShapeHandle, tools: &[ShapeHandle]) -> KResult<Op> {
        let _ = (op, target, tools);
        unsupported!("boolean")
    }
    fn transform(&mut self, shape: ShapeHandle, transform: &Transform) -> KResult<Op> {
        let _ = (shape, transform);
        unsupported!("transform")
    }
    fn pattern(&mut self, shape: ShapeHandle, pattern: &Pattern) -> KResult<Op> {
        let _ = (shape, pattern);
        unsupported!("pattern")
    }

    // ---- queries ----------------------------------------------------------------------------

    fn topology(&self, shape: ShapeHandle) -> KResult<Topology> {
        let _ = shape;
        unsupported!("topology")
    }
    fn face_info(&self, face: FaceId) -> KResult<FaceInfo> {
        let _ = face;
        unsupported!("face_info")
    }
    fn edge_info(&self, edge: EdgeId) -> KResult<EdgeInfo> {
        let _ = edge;
        unsupported!("edge_info")
    }
    /// Display mesh. Takes `&mut self` because backends may cache the triangulation.
    fn tessellate(&mut self, shape: ShapeHandle, tol: &MeshTol) -> KResult<Mesh> {
        let _ = (shape, tol);
        unsupported!("tessellate")
    }
    /// Mass properties for a uniform `density` (mass per mm^3).
    fn mass_properties(&self, shape: ShapeHandle, density: f64) -> KResult<MassProps> {
        let _ = (shape, density);
        unsupported!("mass_properties")
    }
    /// Tight axis-aligned bounding box; `None` for an empty shape.
    fn bounding_box(&self, shape: ShapeHandle) -> KResult<Option<Aabb3>> {
        let _ = shape;
        unsupported!("bounding_box")
    }
    /// Topological and geometric validity check.
    fn is_valid(&self, shape: ShapeHandle) -> KResult<bool> {
        let _ = shape;
        unsupported!("is_valid")
    }

    // ---- exchange ---------------------------------------------------------------------------

    /// Shapes read from STEP (AP203/AP214/AP242) data, one handle per root.
    fn import_step(&mut self, data: &[u8]) -> KResult<Vec<ShapeHandle>> {
        let _ = data;
        unsupported!("import_step")
    }
    /// STEP (AP214) data for `shapes`, in millimetres.
    fn export_step(&self, shapes: &[ShapeHandle]) -> KResult<Vec<u8>> {
        let _ = shapes;
        unsupported!("export_step")
    }

    // ---- lifetime ---------------------------------------------------------------------------

    /// Token that long operations poll; replaced by each call.
    fn set_cancel(&mut self, token: CancelToken) {
        let _ = token;
    }
    /// Frees a shape. Later use of `shape` reports [`KernelError::InvalidHandle`]. Releasing an
    /// unknown handle is a no-op.
    fn release(&mut self, shape: ShapeHandle) {
        let _ = shape;
    }
    /// A second handle to the same shape (same topology and indices), released separately.
    /// Cheap: no geometry is copied.
    fn duplicate(&mut self, shape: ShapeHandle) -> KResult<ShapeHandle> {
        let _ = shape;
        Err(KernelError::Unsupported("duplicate"))
    }
    /// Number of live shapes (leak checks in tests).
    fn live_shapes(&self) -> usize {
        0
    }
}

/// A kernel without geometry: every operation returns [`KernelError::Unsupported`]. Used where no
/// backend is available (wasm builds, UI tests).
#[derive(Debug, Default)]
pub struct NullKernel;

impl Kernel for NullKernel {
    fn name(&self) -> &'static str {
        "null"
    }
    fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_kernel_is_object_safe_and_refuses_everything() {
        let mut k: Box<dyn Kernel> = Box::new(NullKernel);
        let h = ShapeHandle::from_parts(0, 0);
        assert_eq!(k.name(), "null");
        assert_eq!(k.make_box(&Frame::WORLD, Vec3::new(1.0, 1.0, 1.0)), Err(KernelError::Unsupported("make_box")));
        assert_eq!(k.boolean(BoolOp::Cut, h, &[h]), Err(KernelError::Unsupported("boolean")));
        assert_eq!(k.topology(h), Err(KernelError::Unsupported("topology")));
        assert_eq!(k.export_step(&[h]), Err(KernelError::Unsupported("export_step")));
        assert!(matches!(k.tessellate(h, &MeshTol::default()), Err(KernelError::Unsupported(_))));
        k.release(h);
        assert_eq!(k.live_shapes(), 0);
    }

    #[test]
    fn cancel_token_is_shared() {
        let t = CancelToken::new();
        let u = t.clone();
        assert!(!u.is_cancelled());
        t.cancel();
        assert!(u.is_cancelled());
        u.reset();
        assert!(!t.is_cancelled());
    }

    #[test]
    fn errors_render() {
        let e = KernelError::OperationFailed { op: "fillet", reason: "radius too large".into() };
        assert_eq!(e.to_string(), "fillet failed: radius too large");
        assert!(KernelError::Unsupported("loft").to_string().contains("loft"));
    }

    #[test]
    fn handles() {
        let h = ShapeHandle::from_parts(3, 7);
        assert_eq!((h.index(), h.generation()), (3, 7));
        assert_eq!(h.face(2).topo(), TopoId::face(2));
        assert_eq!(h.edge(4).topo().kind, TopoKind::Edge);
    }
}
