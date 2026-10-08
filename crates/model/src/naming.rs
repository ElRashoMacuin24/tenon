//! Persistent face references (docs/persistent-naming.md), M1 scope: faces of extrusions and
//! revolutions and their survival through booleans.
//!
//! During regeneration every body keeps a name for each face: the [`FaceOrigin`] derived from the
//! kernel's operation history. A [`FaceRef`] stores an origin plus a geometric fingerprint;
//! resolving it picks the face with that origin, using the fingerprint only to choose between
//! pieces of a split face. A reference that no longer matches anything is reported, never guessed.

use serde::{Deserialize, Serialize};
use tenon_geom::Vec3;
use tenon_kernel::{EdgeInfo, FaceInfo, History, InputRef, Kernel, Origin, PrimitiveRole, ShapeHandle, SurfaceKind, TopoKind, Topology};
use tenon_sketch::EntityId;

use crate::FeatureId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapEnd {
    Start,
    End,
}

/// How a face came to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FaceOrigin {
    /// The start or end cap of an extrusion or partial revolution.
    Cap { feature: FeatureId, end: CapEnd },
    /// The face swept from one sketch curve by an extrusion or revolution.
    Side { feature: FeatureId, curve: EntityId },
    /// Made by `feature` from the named sub-shape whose key is `source`: a fillet face from an
    /// edge, a corner blend from a vertex, a shell's inner wall from a face, a patterned copy of a
    /// face. `ordinal` tells several faces from one source apart.
    From { feature: FeatureId, source: u64, ordinal: u32 },
}

/// FNV-1a: a fixed, platform-independent hash, so names are the same on every run and machine.
struct Fnv(u64);

impl Fnv {
    fn new(tag: u8) -> Fnv {
        let mut h = Fnv(0xcbf2_9ce4_8422_2325);
        h.bytes(&[tag]);
        h
    }
    fn bytes(&mut self, b: &[u8]) {
        for x in b {
            self.0 ^= u64::from(*x);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    fn u32(&mut self, v: u32) -> &mut Self {
        self.bytes(&v.to_le_bytes());
        self
    }
    fn u64(&mut self, v: u64) -> &mut Self {
        self.bytes(&v.to_le_bytes());
        self
    }
}

impl FaceOrigin {
    pub fn feature(&self) -> FeatureId {
        match self {
            FaceOrigin::Cap { feature, .. } | FaceOrigin::Side { feature, .. } | FaceOrigin::From { feature, .. } => *feature,
        }
    }
    pub fn describe(&self, feature_name: &str) -> String {
        match self {
            FaceOrigin::Cap { end: CapEnd::Start, .. } => format!("start face of {feature_name}"),
            FaceOrigin::Cap { end: CapEnd::End, .. } => format!("end face of {feature_name}"),
            FaceOrigin::Side { curve, .. } => format!("side face of {feature_name} from sketch curve {curve}"),
            FaceOrigin::From { .. } => format!("face made by {feature_name}"),
        }
    }
    /// A stable key of this name (the same on every run), for names derived from it.
    pub fn key(&self) -> u64 {
        match self {
            FaceOrigin::Cap { feature, end } => Fnv::new(1).u32(feature.0).u32(u32::from(*end == CapEnd::End)).0,
            FaceOrigin::Side { feature, curve } => Fnv::new(2).u32(feature.0).u32(curve.0).0,
            FaceOrigin::From { feature, source, ordinal } => Fnv::new(3).u32(feature.0).u64(*source).u32(*ordinal).0,
        }
    }
}

/// Key of the edge between two named faces; the order of the faces does not matter.
pub fn edge_key(a: &FaceOrigin, b: &FaceOrigin) -> u64 {
    let (x, y) = (a.key().min(b.key()), a.key().max(b.key()));
    Fnv::new(4).u64(x).u64(y).0
}

/// Key of a vertex: the names of the faces meeting there, in any order.
pub fn vertex_key(faces: &[FaceOrigin]) -> u64 {
    let mut keys: Vec<u64> = faces.iter().map(FaceOrigin::key).collect();
    keys.sort_unstable();
    keys.dedup();
    let mut h = Fnv::new(5);
    for k in keys {
        h.u64(k);
    }
    h.0
}

/// Geometry of an edge when it was referenced (chooses among edges between the same two faces).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EdgeFingerprint {
    /// Halfway between the edge's end points.
    pub mid: Vec3,
    pub length: f64,
}

impl EdgeFingerprint {
    pub fn of(info: &EdgeInfo) -> EdgeFingerprint {
        EdgeFingerprint { mid: (info.start + info.end) * 0.5, length: info.length }
    }
    pub fn distance(&self, o: &EdgeFingerprint) -> f64 {
        self.mid.dist(o.mid) + (self.length - o.length).abs()
    }
}

/// A persistent reference to an edge: the names of the two faces it joins (sorted by key) and
/// its geometry at the time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EdgeRef {
    pub faces: [FaceOrigin; 2],
    pub fingerprint: EdgeFingerprint,
}

impl EdgeRef {
    pub fn new(a: FaceOrigin, b: FaceOrigin, fingerprint: EdgeFingerprint) -> EdgeRef {
        let faces = if a.key() <= b.key() { [a, b] } else { [b, a] };
        EdgeRef { faces, fingerprint }
    }
    pub fn features(&self) -> Vec<FeatureId> {
        let mut v = vec![self.faces[0].feature(), self.faces[1].feature()];
        v.sort();
        v.dedup();
        v
    }
}

/// The two face names an edge joins (sorted by key), when both faces are named. A seam edge
/// (one face on both sides) pairs the face with itself.
pub fn edge_names(names: &[Option<FaceOrigin>], adjacent: &[u32]) -> Option<[FaceOrigin; 2]> {
    let name = |i: u32| names.get(i as usize).copied().flatten();
    let (a, b) = match adjacent {
        [a] => (name(*a)?, name(*a)?),
        [a, b, ..] => (name(*a)?, name(*b)?),
        [] => return None,
    };
    Some(if a.key() <= b.key() { [a, b] } else { [b, a] })
}

/// Face names of the result of an operation on one body (fillet, chamfer, shell): faces carried
/// over keep their names; new faces are named after the edge, vertex or face they came from.
/// `topo` is the input body's topology (for the faces around its edges and vertices).
pub fn names_of_modify(history: &History, input: &[Option<FaceOrigin>], topo: &Topology, feature: FeatureId, faces: u32) -> Vec<Option<FaceOrigin>> {
    let mut names = vec![None; faces as usize];
    // Faces around each input vertex.
    let mut vertex_faces: Vec<Vec<u32>> = vec![Vec::new(); topo.vertices as usize];
    for (e, verts) in topo.edge_vertices.iter().enumerate() {
        for v in verts {
            if let (Some(row), Some(adj)) = (vertex_faces.get_mut(*v as usize), topo.edge_faces.get(e)) {
                row.extend(adj.iter().copied());
            }
        }
    }
    let mut ordinals: std::collections::BTreeMap<u64, u32> = std::collections::BTreeMap::new();
    for g in &history.generated {
        let Origin::Input(InputRef { input: 0, id }) = g.origin else { continue };
        let source = match id.kind {
            TopoKind::Face => input.get(id.index as usize).copied().flatten().map(|n| n.key()),
            TopoKind::Edge => topo.edge_faces.get(id.index as usize).and_then(|adj| edge_names(input, adj)).map(|[a, b]| edge_key(&a, &b)),
            TopoKind::Vertex => vertex_faces.get(id.index as usize).and_then(|adj| {
                let named: Option<Vec<FaceOrigin>> = adj.iter().map(|f| input.get(*f as usize).copied().flatten()).collect();
                named.map(|n| vertex_key(&n))
            }),
        };
        let Some(source) = source else { continue };
        for t in g.result.iter().filter(|t| t.kind == TopoKind::Face) {
            let ordinal = ordinals.entry(source).or_insert(0);
            if let Some(slot) = names.get_mut(t.index as usize)
                && slot.is_none()
            {
                *slot = Some(FaceOrigin::From { feature, source, ordinal: *ordinal });
                *ordinal += 1;
            }
        }
    }
    for img in &history.images {
        if img.source.input != 0 || img.source.id.kind != TopoKind::Face {
            continue;
        }
        let Some(name) = input.get(img.source.id.index as usize).copied().flatten() else { continue };
        for r in img.result.iter().filter(|r| r.kind == TopoKind::Face) {
            if let Some(slot) = names.get_mut(r.index as usize) {
                *slot = Some(name);
            }
        }
    }
    names
}

/// Finds the edge a reference means among `bodies` (`(shape, names)` pairs): the edges between
/// the two named faces, the nearest fingerprint when there are several. Returns
/// `(body index, edge index)`, or why it cannot be found.
pub fn resolve_edge(eref: &EdgeRef, bodies: &[(ShapeHandle, &[Option<FaceOrigin>])], kernel: &dyn Kernel) -> Result<(usize, u32), String> {
    let mut best: Option<(f64, usize, u32)> = None;
    for (bi, (shape, names)) in bodies.iter().enumerate() {
        let topo = kernel.topology(*shape).map_err(|e| e.to_string())?;
        for (ei, adj) in topo.edge_faces.iter().enumerate() {
            if edge_names(names, adj) != Some(eref.faces) {
                continue;
            }
            let ei = u32::try_from(ei).map_err(|_| "too many edges".to_string())?;
            let d = kernel.edge_info(shape.edge(ei)).map(|i| EdgeFingerprint::of(&i).distance(&eref.fingerprint)).unwrap_or(f64::INFINITY);
            if best.is_none_or(|b| d < b.0) {
                best = Some((d, bi, ei));
            }
        }
    }
    best.map(|(_, b, e)| (b, e)).ok_or_else(|| "the referenced edge no longer exists".into())
}

/// Geometry of a face when it was referenced.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fingerprint {
    pub surface: String,
    /// Plane normal or cylinder/cone/torus axis direction (zero for other surfaces).
    pub direction: Vec3,
    pub centroid: Vec3,
    pub area: f64,
}

impl Fingerprint {
    pub fn of(info: &FaceInfo) -> Fingerprint {
        let (surface, direction) = match &info.surface {
            SurfaceKind::Plane { normal, .. } => ("plane", *normal),
            SurfaceKind::Cylinder { axis, .. } => ("cylinder", axis.dir()),
            SurfaceKind::Cone { axis, .. } => ("cone", axis.dir()),
            SurfaceKind::Sphere { .. } => ("sphere", Vec3::ZERO),
            SurfaceKind::Torus { axis, .. } => ("torus", axis.dir()),
            _ => ("other", Vec3::ZERO),
        };
        Fingerprint { surface: surface.into(), direction, centroid: info.centroid, area: info.area }
    }

    /// Dissimilarity: centroid distance plus a penalty for turned directions and area change.
    pub fn distance(&self, o: &Fingerprint) -> f64 {
        let turn = if self.direction == Vec3::ZERO || o.direction == Vec3::ZERO { 0.0 } else { 1.0 - self.direction.dot(o.direction).abs() };
        let scale = self.area.abs().max(o.area.abs()).sqrt().max(1.0);
        self.centroid.dist(o.centroid) + turn * scale * 10.0 + (self.area - o.area).abs() / scale
    }
}

/// A persistent reference to a face.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaceRef {
    pub origin: FaceOrigin,
    pub fingerprint: Fingerprint,
}

impl FaceRef {
    pub fn feature(&self) -> Option<FeatureId> {
        Some(self.origin.feature())
    }
}

/// Face names of a sweep result, from its history.
pub fn names_of_sweep(history: &History, feature: FeatureId, faces: u32) -> Vec<Option<FaceOrigin>> {
    let mut names = vec![None; faces as usize];
    let mut set = |i: u32, o: FaceOrigin| {
        if let Some(slot) = names.get_mut(i as usize) {
            *slot = Some(o);
        }
    };
    for t in history.roles_of(PrimitiveRole::StartCap) {
        set(t.index, FaceOrigin::Cap { feature, end: CapEnd::Start });
    }
    for t in history.roles_of(PrimitiveRole::EndCap) {
        set(t.index, FaceOrigin::Cap { feature, end: CapEnd::End });
    }
    for g in &history.generated {
        if let Origin::ProfileCurve { tag } = g.origin
            && let Ok(curve) = u32::try_from(tag)
        {
            for t in g.result.iter().filter(|t| t.kind == TopoKind::Face) {
                set(t.index, FaceOrigin::Side { feature, curve: EntityId(curve) });
            }
        }
    }
    names
}

/// The faces of one hole, by the segment of its cross-section that made them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoleFace {
    Top = 1,
    Wall = 2,
    Bottom = 3,
    Point = 4,
    BoreWall = 5,
    BoreFloor = 6,
    Sink = 7,
}

impl HoleFace {
    /// The `source` of the face's name ([`FaceOrigin::From`]): its centre point and segment.
    pub fn source(self, point: EntityId) -> u64 {
        (u64::from(point.0) << 8) | self as u64
    }
}

/// Face names of a revolved tool whose profile curves are tagged with name sources: each face is
/// named after the tag of the curve that swept it.
pub fn names_of_tagged(history: &History, feature: FeatureId, faces: u32) -> Vec<Option<FaceOrigin>> {
    let mut names = vec![None; faces as usize];
    for g in &history.generated {
        if let Origin::ProfileCurve { tag } = g.origin {
            for t in g.result.iter().filter(|t| t.kind == TopoKind::Face) {
                if let Some(slot) = names.get_mut(t.index as usize) {
                    *slot = Some(FaceOrigin::From { feature, source: tag, ordinal: 0 });
                }
            }
        }
    }
    names
}

/// Face names of a boolean result: each result face inherits the name of the input face it came
/// from. `inputs[i]` are the names of input `i` (target first, then tools).
pub fn names_of_boolean(history: &History, inputs: &[&[Option<FaceOrigin>]], faces: u32) -> Vec<Option<FaceOrigin>> {
    let mut names = vec![None; faces as usize];
    for img in &history.images {
        if img.source.id.kind != TopoKind::Face {
            continue;
        }
        let Some(name) = inputs.get(img.source.input as usize).and_then(|n| n.get(img.source.id.index as usize)).copied().flatten() else {
            continue;
        };
        for r in img.result.iter().filter(|r| r.kind == TopoKind::Face) {
            if let Some(slot) = names.get_mut(r.index as usize) {
                *slot = Some(name);
            }
        }
    }
    names
}

/// Finds the face a reference means among `bodies` (`(shape, names)` pairs). Returns
/// `(body index, face index)`, or why it cannot be found.
pub fn resolve(fref: &FaceRef, bodies: &[(ShapeHandle, &[Option<FaceOrigin>])], kernel: &dyn Kernel) -> Result<(usize, u32), String> {
    let mut best: Option<(f64, usize, u32)> = None;
    let mut count = 0;
    for (bi, (shape, names)) in bodies.iter().enumerate() {
        for (fi, name) in names.iter().enumerate() {
            if *name != Some(fref.origin) {
                continue;
            }
            let fi = u32::try_from(fi).map_err(|_| "too many faces".to_string())?;
            count += 1;
            let d = kernel.face_info(shape.face(fi)).map(|info| Fingerprint::of(&info).distance(&fref.fingerprint)).unwrap_or(f64::INFINITY);
            if best.is_none_or(|b| d < b.0) {
                best = Some((d, bi, fi));
            }
        }
    }
    match best {
        Some((_, b, f)) => Ok((b, f)),
        None if count == 0 => Err("the referenced face no longer exists".into()),
        None => Err("the referenced face is ambiguous".into()),
    }
}
