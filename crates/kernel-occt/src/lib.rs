//! Tenon kernel backend over OpenCASCADE Technology 8.
//!
//! OCCT (LGPL-2.1 with the Open CASCADE exception) is linked dynamically; see NOTICE. This is
//! the only crate in the workspace allowed to use `unsafe`, and the only `unsafe` it contains is
//! the cxx bridge in [`ffi`] plus one documented `Send` impl. The C++ side (`shim/`) catches every
//! exception, so failures arrive here as `cxx::Exception` and leave as [`KernelError`].
//!
//! Shapes live in a generational arena; [`ShapeHandle`]s index it.

mod ffi;

use std::sync::Mutex;

use cxx::UniquePtr;
use ffi::bridge as sys;
use tenon_geom::{Aabb3, Axis, Frame, Vec2, Vec3, tol};
use tenon_kernel::{
    AngleExtent, BoolOp, CancelToken, ChamferSpec, Curve2, CurveKind, EdgeId, EdgeInfo, EdgePolyline, Extent, FaceId, FaceInfo, FaceRange, Generated,
    History, Image, InputRef, KResult, Kernel, KernelError, MassProps, Mesh, MeshTol, Op, Origin, PrimitiveRole, Profile, ShapeHandle, ShapeKind,
    SurfaceKind, TopoId, TopoKind, Topology, Transform, check,
};

/// OCCT's STEP translator keeps global state; exchange calls are serialised process-wide.
static EXCHANGE_LOCK: Mutex<()> = Mutex::new(());

struct Slot {
    generation: u32,
    shape: Option<UniquePtr<sys::Shape>>,
}

/// The OpenCASCADE kernel. Create one per thread that models; it is `Send` but not `Sync`.
pub struct OcctKernel {
    slots: Vec<Slot>,
    free: Vec<u32>,
    cancel: CancelToken,
}

impl Default for OcctKernel {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for OcctKernel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OcctKernel").field("live_shapes", &self.live_shapes()).finish()
    }
}

fn failed(op: &'static str) -> impl Fn(cxx::Exception) -> KernelError {
    move |e| KernelError::OperationFailed { op, reason: e.what().to_owned() }
}

fn exchange(e: cxx::Exception) -> KernelError {
    KernelError::Exchange(e.what().to_owned())
}

fn v3(v: Vec3) -> sys::V3 {
    sys::V3 { x: v.x, y: v.y, z: v.z }
}

fn vec3(v: &sys::V3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

fn frame3(f: &Frame) -> sys::Frame3 {
    sys::Frame3 { origin: v3(f.origin()), x_dir: v3(f.x()), z_dir: v3(f.z()) }
}

fn axis_frame(what: &str, axis: &Axis) -> KResult<sys::Frame3> {
    check::point(what, axis.origin())?;
    let f = Frame::from_normal(axis.origin(), axis.dir()).ok_or_else(|| KernelError::InvalidInput(format!("{what} has no valid direction")))?;
    Ok(frame3(&f))
}

fn topo_kind(code: u8) -> Option<TopoKind> {
    match code {
        0 => Some(TopoKind::Vertex),
        1 => Some(TopoKind::Edge),
        2 => Some(TopoKind::Face),
        _ => None,
    }
}

fn role(code: u8) -> Option<PrimitiveRole> {
    use PrimitiveRole::*;
    [BoxXMin, BoxXMax, BoxYMin, BoxYMax, BoxZMin, BoxZMax, Lateral, Bottom, Top, StartCap, EndCap].get(usize::from(code)).copied()
}

/// Most curves a profile may have (hostile-input cap).
const MAX_PROFILE_CURVES: usize = 20_000;
/// Most poles of one profile spline.
const MAX_SPLINE_POLES: usize = 2_000;

fn bad(msg: impl Into<String>) -> KernelError {
    KernelError::InvalidInput(msg.into())
}

fn finite2(what: &str, p: Vec2) -> KResult<Vec2> {
    if tol::is_valid_coord(p.x) && tol::is_valid_coord(p.y) { Ok(p) } else { Err(bad(format!("{what} must be finite and in range, got {p:?}"))) }
}

/// Validates a profile and flattens it for the shim.
fn profile_in(p: &Profile) -> KResult<sys::ProfileIn> {
    check::point("profile origin", p.frame.origin())?;
    if p.regions.is_empty() {
        return Err(bad("the profile has no regions"));
    }
    let mut out = sys::ProfileIn { frame: frame3(&p.frame), curves: Vec::new(), poles: Vec::new() };
    for (ri, region) in p.regions.iter().enumerate() {
        let region_index = u32::try_from(ri).map_err(|_| bad("too many regions"))?;
        for (li, lp) in std::iter::once(&region.outer).chain(&region.holes).enumerate() {
            if lp.curves.is_empty() {
                return Err(bad("a profile loop has no curves"));
            }
            let loop_index = u32::try_from(li).map_err(|_| bad("too many loops"))?;
            for tc in &lp.curves {
                if out.curves.len() >= MAX_PROFILE_CURVES {
                    return Err(bad(format!("a profile may have at most {MAX_PROFILE_CURVES} curves")));
                }
                let mut c = sys::CurveIn { tag: tc.tag, region: region_index, loop_index, ..Default::default() };
                match &tc.curve {
                    Curve2::Line { start, end } => {
                        let (a, b) = (finite2("line start", *start)?, finite2("line end", *end)?);
                        (c.kind, c.x0, c.y0, c.x1, c.y1) = (0, a.x, a.y, b.x, b.y);
                    }
                    Curve2::Arc { center, radius, start_angle, end_angle } => {
                        let cc = finite2("arc centre", *center)?;
                        check::size("arc radius", *radius)?;
                        if !(start_angle.is_finite() && end_angle.is_finite()) {
                            return Err(bad("arc angles must be finite"));
                        }
                        (c.kind, c.cx, c.cy, c.r, c.a0, c.a1) = (1, cc.x, cc.y, *radius, *start_angle, *end_angle);
                    }
                    Curve2::Circle { center, radius } => {
                        let cc = finite2("circle centre", *center)?;
                        check::size("circle radius", *radius)?;
                        (c.kind, c.cx, c.cy, c.r) = (2, cc.x, cc.y, *radius);
                    }
                    Curve2::BSpline { poles, degree } => {
                        if !(1..=8).contains(degree) || poles.len() <= *degree as usize || poles.len() > MAX_SPLINE_POLES {
                            return Err(bad(format!("a spline needs degree 1..=8 and more poles than its degree (at most {MAX_SPLINE_POLES})")));
                        }
                        c.kind = 3;
                        c.degree = *degree;
                        c.pole_start = u32::try_from(out.poles.len() / 2).map_err(|_| bad("too many poles"))?;
                        c.pole_count = u32::try_from(poles.len()).map_err(|_| bad("too many poles"))?;
                        for p in poles {
                            let p = finite2("spline pole", *p)?;
                            out.poles.extend([p.x, p.y]);
                        }
                    }
                }
                out.curves.push(c);
            }
        }
    }
    Ok(out)
}

fn non_zero_length(what: &str, v: f64) -> KResult<f64> {
    check::size(what, v.abs()).map(|_| v)
}

fn convert_history(h: sys::HistoryOut) -> History {
    let roles = h.roles.iter().filter_map(|r| Some((role(r.role)?, TopoId::face(r.face)))).collect();
    let images = h
        .images
        .iter()
        .filter_map(|e| {
            let kind = topo_kind(e.kind)?;
            let source = InputRef { input: e.input, id: TopoId { kind, index: e.index } };
            Some(Image { source, result: e.images.iter().map(|&index| TopoId { kind, index }).collect() })
        })
        .collect();
    let mut generated: Vec<Generated> = Vec::new();
    let mut add = |origin: Origin, id: TopoId| match generated.iter_mut().find(|x| x.origin == origin) {
        Some(x) if !x.result.contains(&id) => x.result.push(id),
        Some(_) => {}
        None => generated.push(Generated { origin, result: vec![id] }),
    };
    for g in &h.generated {
        if let (Some(kind), Some(gen_kind)) = (topo_kind(g.kind), topo_kind(g.gen_kind)) {
            add(Origin::Input(InputRef { input: g.input, id: TopoId { kind, index: g.index } }), TopoId { kind: gen_kind, index: g.gen_index });
        }
    }
    for t in &h.tagged {
        if let Some(kind) = topo_kind(t.gen_kind) {
            add(Origin::ProfileCurve { tag: t.tag }, TopoId { kind, index: t.gen_index });
        }
    }
    History { images, generated, roles }
}

/// Rows of a compressed adjacency table.
fn csr(offsets: &[u32], values: &[u32]) -> Vec<Vec<u32>> {
    offsets.windows(2).map(|w| values.get(w[0] as usize..w[1] as usize).map(<[u32]>::to_vec).unwrap_or_default()).collect()
}

fn shape_kind(code: u8) -> ShapeKind {
    match code {
        0 => ShapeKind::Compound,
        1 => ShapeKind::CompSolid,
        2 => ShapeKind::Solid,
        3 => ShapeKind::Shell,
        4 => ShapeKind::Face,
        5 => ShapeKind::Wire,
        6 => ShapeKind::Edge,
        _ => ShapeKind::Vertex,
    }
}

/// Edge indices, checking that every edge belongs to `body`.
fn own_edges(body: ShapeHandle, edges: &[EdgeId]) -> KResult<Vec<u32>> {
    if edges.is_empty() {
        return Err(bad("select at least one edge"));
    }
    if edges.len() > 10_000 {
        return Err(bad("too many edges"));
    }
    edges.iter().map(|e| if e.shape == body { Ok(e.index) } else { Err(bad("an edge is not on the body")) }).collect()
}

fn axis_or_z(origin: &sys::V3, dir: &sys::V3) -> Axis {
    Axis::new(vec3(origin), vec3(dir)).unwrap_or(Axis::Z)
}

impl OcctKernel {
    pub fn new() -> Self {
        OcctKernel { slots: Vec::new(), free: Vec::new(), cancel: CancelToken::new() }
    }

    /// Version of the linked OpenCASCADE library, e.g. "8.0.1".
    pub fn occt_version() -> KResult<String> {
        sys::occt_version().map_err(failed("occt_version"))
    }

    fn insert(&mut self, shape: UniquePtr<sys::Shape>) -> KResult<ShapeHandle> {
        if shape.is_null() {
            return Err(KernelError::Backend("the backend returned a null shape".into()));
        }
        if let Some(index) = self.free.pop()
            && let Some(slot) = self.slots.get_mut(index as usize)
        {
            slot.shape = Some(shape);
            return Ok(ShapeHandle::from_parts(index, slot.generation));
        }
        let index = u32::try_from(self.slots.len()).map_err(|_| KernelError::Backend("shape arena is full".into()))?;
        self.slots.push(Slot { generation: 0, shape: Some(shape) });
        Ok(ShapeHandle::from_parts(index, 0))
    }

    fn get(&self, h: ShapeHandle) -> KResult<&sys::Shape> {
        self.slots
            .get(h.index() as usize)
            .filter(|s| s.generation == h.generation())
            .and_then(|s| s.shape.as_ref())
            .and_then(|p| p.as_ref())
            .ok_or(KernelError::InvalidHandle)
    }

    fn not_cancelled(&self) -> KResult<()> {
        if self.cancel.is_cancelled() { Err(KernelError::Cancelled) } else { Ok(()) }
    }

    fn finish(&mut self, shape: UniquePtr<sys::Shape>, hist: sys::HistoryOut) -> KResult<Op> {
        let shape = self.insert(shape)?;
        Ok(Op { shape, history: convert_history(hist) })
    }

    fn shape_list(&self, shapes: &[ShapeHandle]) -> KResult<UniquePtr<sys::ShapeList>> {
        let mut list = sys::new_shape_list().map_err(failed("shape list"))?;
        if list.is_null() {
            return Err(KernelError::Backend("could not allocate a shape list".into()));
        }
        for h in shapes {
            let s = self.get(*h)?;
            sys::shape_list_push(list.pin_mut(), s).map_err(failed("shape list"))?;
        }
        Ok(list)
    }
}

impl Kernel for OcctKernel {
    fn name(&self) -> &'static str {
        "occt"
    }

    fn version(&self) -> String {
        format!("OpenCASCADE {}", Self::occt_version().unwrap_or_else(|_| "unknown".into()))
    }

    fn make_box(&mut self, frame: &Frame, size: Vec3) -> KResult<Op> {
        self.not_cancelled()?;
        check::point("box origin", frame.origin())?;
        let (dx, dy, dz) = (check::size("box length", size.x)?, check::size("box width", size.y)?, check::size("box height", size.z)?);
        let mut hist = sys::HistoryOut::default();
        let shape = sys::make_box(&frame3(frame), dx, dy, dz, &mut hist).map_err(failed("make_box"))?;
        self.finish(shape, hist)
    }

    fn make_cylinder(&mut self, axis: &Axis, radius: f64, height: f64) -> KResult<Op> {
        self.not_cancelled()?;
        let f = axis_frame("cylinder axis", axis)?;
        let (r, h) = (check::size("cylinder radius", radius)?, check::size("cylinder height", height)?);
        let mut hist = sys::HistoryOut::default();
        let shape = sys::make_cylinder(&f, r, h, &mut hist).map_err(failed("make_cylinder"))?;
        self.finish(shape, hist)
    }

    fn make_cone(&mut self, axis: &Axis, r1: f64, r2: f64, height: f64) -> KResult<Op> {
        self.not_cancelled()?;
        let f = axis_frame("cone axis", axis)?;
        let h = check::size("cone height", height)?;
        let radius_ok = |r: f64| r == 0.0 || tol::is_valid_size(r);
        if !radius_ok(r1) || !radius_ok(r2) || (r1 - r2).abs() <= tol::MIN_SIZE {
            return Err(KernelError::InvalidInput(format!(
                "cone radii must be 0 or between {} and {} mm, and differ (use a cylinder for equal radii); got {r1} and {r2}",
                tol::MIN_SIZE,
                tol::MAX_SIZE
            )));
        }
        let mut hist = sys::HistoryOut::default();
        let shape = sys::make_cone(&f, r1, r2, h, &mut hist).map_err(failed("make_cone"))?;
        self.finish(shape, hist)
    }

    fn make_sphere(&mut self, center: Vec3, radius: f64) -> KResult<Op> {
        self.not_cancelled()?;
        check::point("sphere centre", center)?;
        let r = check::size("sphere radius", radius)?;
        let shape = sys::make_sphere(&v3(center), r).map_err(failed("make_sphere"))?;
        self.finish(shape, sys::HistoryOut::default())
    }

    fn make_torus(&mut self, axis: &Axis, major_radius: f64, minor_radius: f64) -> KResult<Op> {
        self.not_cancelled()?;
        let f = axis_frame("torus axis", axis)?;
        let (big, small) = (check::size("torus major radius", major_radius)?, check::size("torus minor radius", minor_radius)?);
        if small >= big {
            return Err(KernelError::InvalidInput(format!("torus minor radius {small} must be smaller than the major radius {big}")));
        }
        let shape = sys::make_torus(&f, big, small).map_err(failed("make_torus"))?;
        self.finish(shape, sys::HistoryOut::default())
    }

    fn boolean(&mut self, op: BoolOp, target: ShapeHandle, tools: &[ShapeHandle]) -> KResult<Op> {
        self.not_cancelled()?;
        if tools.is_empty() {
            return Err(KernelError::InvalidInput("a boolean needs at least one tool".into()));
        }
        let code = match op {
            BoolOp::Union => 0,
            BoolOp::Cut => 1,
            BoolOp::Intersect => 2,
        };
        let list = self.shape_list(tools)?;
        let mut hist = sys::HistoryOut::default();
        let shape = sys::boolean_op(code, self.get(target)?, &list, &mut hist).map_err(failed("boolean"))?;
        self.finish(shape, hist)
    }

    fn make_face(&mut self, profile: &Profile) -> KResult<Op> {
        self.not_cancelled()?;
        let p = profile_in(profile)?;
        let mut hist = sys::HistoryOut::default();
        let shape = sys::make_face(&p, 0.0, &mut hist).map_err(failed("make_face"))?;
        self.finish(shape, hist)
    }

    fn extrude(&mut self, profile: &Profile, extent: &Extent, taper: Option<f64>) -> KResult<Op> {
        self.not_cancelled()?;
        if taper.is_some_and(|t| t.abs() > tol::ANGULAR) {
            return Err(KernelError::Unsupported("tapered extrude"));
        }
        let (start, length) = match extent {
            Extent::Distance(d) => (0.0, non_zero_length("extrude distance", *d)?),
            Extent::Symmetric(d) => {
                let d = check::size("extrude distance", *d)?;
                (-d / 2.0, d)
            }
            Extent::TwoSided { forward, backward } => {
                if !(forward.is_finite() && backward.is_finite() && *forward >= 0.0 && *backward >= 0.0) {
                    return Err(bad("two-sided extrude distances must be finite and non-negative"));
                }
                (-*backward, check::size("extrude total distance", forward + backward)?)
            }
            Extent::ThroughAll { .. } => return Err(KernelError::Unsupported("through-all extrude without a target (the model resolves it)")),
            Extent::ToFace(_) => return Err(KernelError::Unsupported("extrude to face")),
        };
        let p = profile_in(profile)?;
        let mut hist = sys::HistoryOut::default();
        let shape = sys::extrude(&p, start, length, &mut hist).map_err(failed("extrude"))?;
        self.finish(shape, hist)
    }

    fn revolve(&mut self, profile: &Profile, axis: &Axis, angle: &AngleExtent) -> KResult<Op> {
        self.not_cancelled()?;
        check::point("revolve axis origin", axis.origin())?;
        let tau = std::f64::consts::TAU;
        let ok = |a: f64| a.is_finite() && a.abs() > tol::ANGULAR && a.abs() <= tau + tol::ANGULAR;
        let (start, sweep, full) = match angle {
            AngleExtent::Full => (0.0, tau, true),
            AngleExtent::Angle(a) if ok(*a) => (0.0, *a, (a.abs() - tau).abs() <= tol::ANGULAR),
            AngleExtent::Symmetric(a) if ok(*a) => (-a / 2.0, *a, (a.abs() - tau).abs() <= tol::ANGULAR),
            AngleExtent::TwoSided { forward, backward } if forward.is_finite() && backward.is_finite() && ok(forward + backward) => {
                (-*backward, forward + backward, (forward + backward - tau).abs() <= tol::ANGULAR)
            }
            _ => return Err(bad("revolve angles must be finite, non-zero and at most a full turn")),
        };
        let p = profile_in(profile)?;
        let mut hist = sys::HistoryOut::default();
        let shape = sys::revolve(&p, &v3(axis.origin()), &v3(axis.dir()), if full { 0.0 } else { start }, sweep, full, &mut hist)
            .map_err(failed("revolve"))?;
        self.finish(shape, hist)
    }

    fn transform(&mut self, shape: ShapeHandle, transform: &Transform) -> KResult<Op> {
        self.not_cancelled()?;
        let zero = sys::V3::default();
        let (kind, a, b, value) = match transform {
            Transform::Translate(v) => (0, v3(check::point("translation", *v)?), zero, 0.0),
            Transform::Rotate { axis, angle } => {
                if !angle.is_finite() {
                    return Err(bad("rotation angle must be finite"));
                }
                (1, v3(check::point("rotation axis origin", axis.origin())?), v3(axis.dir()), *angle)
            }
            Transform::Mirror { plane } => (2, v3(check::point("mirror plane origin", plane.origin())?), v3(plane.z()), 0.0),
            Transform::Scale { center, factor } => {
                if !(factor.is_finite() && *factor > 1e-6 && *factor < 1e6) {
                    return Err(bad("scale factor must be between 1e-6 and 1e6"));
                }
                (3, v3(check::point("scale centre", *center)?), zero, *factor)
            }
        };
        let mut hist = sys::HistoryOut::default();
        let out = sys::transform(self.get(shape)?, kind, &a, &b, value, &mut hist).map_err(failed("transform"))?;
        self.finish(out, hist)
    }

    fn fillet(&mut self, body: ShapeHandle, edges: &[EdgeId], radius: f64) -> KResult<Op> {
        self.not_cancelled()?;
        let r = check::size("fillet radius", radius)?;
        let idx = own_edges(body, edges)?;
        let mut hist = sys::HistoryOut::default();
        let shape = sys::fillet(self.get(body)?, &idx, r, &mut hist).map_err(failed("fillet"))?;
        self.finish(shape, hist)
    }

    fn chamfer(&mut self, body: ShapeHandle, edges: &[EdgeId], spec: &ChamferSpec) -> KResult<Op> {
        self.not_cancelled()?;
        let idx = own_edges(body, edges)?;
        let own_face = |f: &FaceId| if f.shape == body { Ok(f.index) } else { Err(bad("the chamfer reference face is not on the body")) };
        let (kind, a, b, reference) = match spec {
            ChamferSpec::Equal(d) => (0, check::size("chamfer distance", *d)?, 0.0, 0),
            ChamferSpec::TwoDistances { d1, d2, reference } => {
                (1, check::size("chamfer distance", *d1)?, check::size("chamfer distance", *d2)?, own_face(reference)?)
            }
            ChamferSpec::DistanceAngle { distance, angle, reference } => {
                if !(angle.is_finite() && *angle > 1e-3 && *angle < std::f64::consts::FRAC_PI_2 - 1e-3) {
                    return Err(bad("chamfer angle must be between 0 and 90 degrees"));
                }
                (2, check::size("chamfer distance", *distance)?, *angle, own_face(reference)?)
            }
        };
        let mut hist = sys::HistoryOut::default();
        let shape = sys::chamfer(self.get(body)?, &idx, kind, a, b, reference, &mut hist).map_err(failed("chamfer"))?;
        self.finish(shape, hist)
    }

    fn shell(&mut self, body: ShapeHandle, open_faces: &[FaceId], thickness: f64) -> KResult<Op> {
        self.not_cancelled()?;
        let t = check::size("shell thickness", thickness.abs())?;
        if open_faces.iter().any(|f| f.shape != body) {
            return Err(bad("a face to remove is not on the body"));
        }
        let idx: Vec<u32> = open_faces.iter().map(|f| f.index).collect();
        let mut hist = sys::HistoryOut::default();
        // Positive thickness keeps the outer skin: the wall grows inwards.
        let offset = if thickness >= 0.0 { -t } else { t };
        let shape = sys::shell(self.get(body)?, &idx, offset, &mut hist).map_err(failed("shell"))?;
        self.finish(shape, hist)
    }

    fn topology(&self, shape: ShapeHandle) -> KResult<Topology> {
        let mut t = sys::TopoOut::default();
        sys::topology(self.get(shape)?, &mut t).map_err(failed("topology"))?;
        Ok(Topology {
            kind: shape_kind(t.kind),
            solids: t.solids,
            shells: t.shells,
            faces: t.faces,
            edges: t.edges,
            vertices: t.vertices,
            face_edges: csr(&t.face_edge_offsets, &t.face_edges),
            edge_faces: csr(&t.edge_face_offsets, &t.edge_faces),
            edge_vertices: csr(&t.edge_vertex_offsets, &t.edge_vertices),
        })
    }

    fn face_info(&self, face: FaceId) -> KResult<FaceInfo> {
        let s = self.get(face.shape)?;
        let count = sys::face_count(s);
        if face.index >= count {
            return Err(KernelError::InvalidInput(format!("face {} does not exist (the shape has {count} faces)", face.index)));
        }
        let mut o = sys::FaceOut::default();
        sys::face_info(s, face.index, &mut o).map_err(failed("face_info"))?;
        let surface = match o.kind {
            0 => SurfaceKind::Plane { origin: vec3(&o.origin), normal: vec3(&o.dir) },
            1 => SurfaceKind::Cylinder { axis: axis_or_z(&o.origin, &o.dir), radius: o.radius },
            2 => SurfaceKind::Cone { axis: axis_or_z(&o.origin, &o.dir), half_angle: o.angle, ref_radius: o.radius },
            3 => SurfaceKind::Sphere { center: vec3(&o.origin), radius: o.radius },
            4 => SurfaceKind::Torus { axis: axis_or_z(&o.origin, &o.dir), major_radius: o.radius, minor_radius: o.radius2 },
            5 => SurfaceKind::BSpline,
            6 => SurfaceKind::Bezier,
            7 => SurfaceKind::Revolution,
            8 => SurfaceKind::Extrusion,
            9 => SurfaceKind::Offset,
            _ => SurfaceKind::Other,
        };
        Ok(FaceInfo { surface, area: o.area, centroid: vec3(&o.centroid), reversed: o.reversed })
    }

    fn edge_info(&self, edge: EdgeId) -> KResult<EdgeInfo> {
        let s = self.get(edge.shape)?;
        let count = sys::edge_count(s);
        if edge.index >= count {
            return Err(KernelError::InvalidInput(format!("edge {} does not exist (the shape has {count} edges)", edge.index)));
        }
        let mut o = sys::EdgeOut::default();
        sys::edge_info(s, edge.index, &mut o).map_err(failed("edge_info"))?;
        let curve = match o.kind {
            0 => CurveKind::Line { origin: vec3(&o.origin), dir: vec3(&o.dir) },
            1 => CurveKind::Circle { axis: axis_or_z(&o.origin, &o.dir), radius: o.radius },
            2 => CurveKind::Ellipse,
            3 => CurveKind::Hyperbola,
            4 => CurveKind::Parabola,
            5 => CurveKind::BSpline,
            6 => CurveKind::Bezier,
            7 => CurveKind::Offset,
            _ => CurveKind::Other,
        };
        Ok(EdgeInfo { curve, length: o.length, start: vec3(&o.start), end: vec3(&o.end), degenerate: o.degenerate })
    }

    fn tessellate(&mut self, shape: ShapeHandle, mesh_tol: &MeshTol) -> KResult<Mesh> {
        self.not_cancelled()?;
        check::mesh_tol(mesh_tol)?;
        let mut o = sys::MeshOut::default();
        sys::tessellate(self.get(shape)?, mesh_tol.linear, mesh_tol.angular, &mut o).map_err(failed("tessellate"))?;
        let triples = |v: &[f32]| v.as_chunks::<3>().0.to_vec();
        let faces = o.face_ranges.as_chunks::<3>().0.iter().map(|&[face, first, count]| FaceRange { face, first, count }).collect();
        let edge_points = triples(&o.edge_points);
        let edges = o
            .edge_ranges
            .as_chunks::<3>()
            .0
            .iter()
            .map(|&[edge, first, count]| EdgePolyline {
                edge,
                points: edge_points
                    .get(first as usize..(first as usize).saturating_add(count as usize))
                    .map(<[[f32; 3]]>::to_vec)
                    .unwrap_or_default(),
            })
            .collect();
        Ok(Mesh { positions: triples(&o.positions), normals: triples(&o.normals), indices: o.indices.to_vec(), faces, edges })
    }

    fn mass_properties(&self, shape: ShapeHandle, density: f64) -> KResult<MassProps> {
        let density = check::density(density)?;
        let mut o = sys::MassOut::default();
        sys::mass_properties(self.get(shape)?, &mut o).map_err(failed("mass_properties"))?;
        let mut inertia = [[0.0; 3]; 3];
        for (i, v) in o.inertia.iter().take(9).enumerate() {
            inertia[i / 3][i % 3] = v * density;
        }
        Ok(MassProps { volume: o.volume, area: o.area, mass: o.volume * density, center_of_mass: vec3(&o.center), inertia })
    }

    fn bounding_box(&self, shape: ShapeHandle) -> KResult<Option<Aabb3>> {
        let mut o = sys::BoxOut::default();
        sys::bounding_box(self.get(shape)?, &mut o).map_err(failed("bounding_box"))?;
        Ok((!o.is_void).then(|| Aabb3::new(vec3(&o.min), vec3(&o.max))))
    }

    fn is_valid(&self, shape: ShapeHandle) -> KResult<bool> {
        sys::is_valid(self.get(shape)?).map_err(failed("is_valid"))
    }

    fn import_step(&mut self, data: &[u8]) -> KResult<Vec<ShapeHandle>> {
        self.not_cancelled()?;
        if data.is_empty() {
            return Err(KernelError::InvalidInput("no STEP data".into()));
        }
        let list = {
            let _guard = EXCHANGE_LOCK.lock().map_err(|_| KernelError::Backend("exchange lock poisoned".into()))?;
            sys::import_step(data).map_err(exchange)?
        };
        let mut out = Vec::new();
        for i in 0..sys::shape_list_len(&list) {
            let s = sys::shape_list_get(&list, i).map_err(exchange)?;
            out.push(self.insert(s)?);
        }
        Ok(out)
    }

    fn export_step(&self, shapes: &[ShapeHandle]) -> KResult<Vec<u8>> {
        self.not_cancelled()?;
        if shapes.is_empty() {
            return Err(KernelError::InvalidInput("nothing to export".into()));
        }
        let list = self.shape_list(shapes)?;
        let _guard = EXCHANGE_LOCK.lock().map_err(|_| KernelError::Backend("exchange lock poisoned".into()))?;
        Ok(sys::export_step(&list).map_err(exchange)?.to_vec())
    }

    fn set_cancel(&mut self, token: CancelToken) {
        self.cancel = token;
    }

    fn release(&mut self, shape: ShapeHandle) {
        if let Some(slot) = self.slots.get_mut(shape.index() as usize)
            && slot.generation == shape.generation()
            && slot.shape.is_some()
        {
            slot.shape = None;
            slot.generation = slot.generation.wrapping_add(1);
            self.free.push(shape.index());
        }
    }

    fn live_shapes(&self) -> usize {
        self.slots.iter().filter(|s| s.shape.is_some()).count()
    }
}
