//! Persistent face references (docs/persistent-naming.md), M1 scope: faces of extrusions and
//! revolutions and their survival through booleans.
//!
//! During regeneration every body keeps a name for each face: the [`FaceOrigin`] derived from the
//! kernel's operation history. A [`FaceRef`] stores an origin plus a geometric fingerprint;
//! resolving it picks the face with that origin, using the fingerprint only to choose between
//! pieces of a split face. A reference that no longer matches anything is reported, never guessed.

use serde::{Deserialize, Serialize};
use tenon_geom::Vec3;
use tenon_kernel::{FaceInfo, History, Kernel, Origin, PrimitiveRole, ShapeHandle, SurfaceKind, TopoKind};
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
}

impl FaceOrigin {
    pub fn feature(&self) -> FeatureId {
        match self {
            FaceOrigin::Cap { feature, .. } | FaceOrigin::Side { feature, .. } => *feature,
        }
    }
    pub fn describe(&self, feature_name: &str) -> String {
        match self {
            FaceOrigin::Cap { end: CapEnd::Start, .. } => format!("start face of {feature_name}"),
            FaceOrigin::Cap { end: CapEnd::End, .. } => format!("end face of {feature_name}"),
            FaceOrigin::Side { curve, .. } => format!("side face of {feature_name} from sketch curve {curve}"),
        }
    }
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
