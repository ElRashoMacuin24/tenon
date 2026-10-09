//! Repairing broken references (DEC-034): which references of a failing feature no longer find
//! their face or edge, the nearest faces or edges to use instead, and the edit that puts one in.
//!
//! Everything here reads a [`Scene`]: after a failure it shows the part as it stood just before
//! the failing feature, which is where that feature's references must be found.

use serde_json::Value;
use tenon_geom::Vec3;

use crate::document::{Document, FeatureId};
use crate::naming::{EdgeRef, FaceRef, Fingerprint};
use crate::regen::Scene;

/// What a reference points at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefKind {
    Edge,
    Face,
}

/// A replacement for a broken reference: a face or edge of the scene, and the reference to it.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub body: usize,
    /// The face's or edge's index in its body.
    pub index: u32,
    /// The reference to store (an `EdgeRef` or a `FaceRef`).
    pub reference: Value,
    /// How far it is from where the broken reference was (mm; smaller is nearer).
    pub distance: f64,
}

/// A reference of a feature that no longer finds its face or edge.
#[derive(Clone, Debug, PartialEq)]
pub struct Broken {
    pub feature: FeatureId,
    /// Where the reference is in the feature's definition (a JSON pointer: `/edges/0`).
    pub path: String,
    pub kind: RefKind,
    /// The nearest faces or edges of the same sort, nearest first (at most [`CANDIDATES`]).
    pub candidates: Vec<Candidate>,
}

/// How many replacements are offered.
pub const CANDIDATES: usize = 3;

/// The references of `feature` that do not find their face or edge in `scene`, each with the
/// nearest replacements.
pub fn broken(doc: &Document, scene: &Scene, feature: FeatureId) -> Vec<Broken> {
    let Some(f) = doc.feature(feature) else { return Vec::new() };
    let Ok(kind) = serde_json::to_value(&f.kind) else { return Vec::new() };
    let mut refs = Vec::new();
    references(&kind, &mut String::new(), &mut refs);
    let mut out = Vec::new();
    for (path, r) in refs {
        match r {
            Found::Edge(e) if !edge_exists(scene, &e) => {
                out.push(Broken { feature, path, kind: RefKind::Edge, candidates: edge_candidates(scene, &e) })
            }
            Found::Face(fr) if !face_exists(scene, &fr) => {
                out.push(Broken { feature, path, kind: RefKind::Face, candidates: face_candidates(scene, &fr) })
            }
            _ => {}
        }
    }
    out
}

/// Puts `with` (a reference) in place of the one at `path` in `feature`'s definition. The
/// caller makes it one undoable edit.
pub fn replace(doc: &mut Document, feature: FeatureId, path: &str, with: Value) -> Result<(), String> {
    let f = doc.feature_mut(feature).ok_or_else(|| format!("{feature} does not exist"))?;
    let mut kind = serde_json::to_value(&f.kind).map_err(|e| e.to_string())?;
    let slot = kind.pointer_mut(path).ok_or_else(|| format!("{} has no reference at {path}", f.name))?;
    let same_sort = |v: &Value| (serde_json::from_value::<EdgeRef>(v.clone()).is_ok(), serde_json::from_value::<FaceRef>(v.clone()).is_ok());
    if same_sort(slot) != same_sort(&with) || same_sort(&with) == (false, false) {
        return Err(format!("{} needs {} there", f.name, if same_sort(slot).0 { "an edge" } else { "a face" }));
    }
    *slot = with;
    f.kind = serde_json::from_value(kind).map_err(|e| e.to_string())?;
    doc.validate()
}

enum Found {
    Edge(EdgeRef),
    Face(FaceRef),
}

/// Every edge and face reference in a feature's definition, with its path.
fn references(v: &Value, path: &mut String, out: &mut Vec<(String, Found)>) {
    match v {
        Value::Object(o) => {
            if o.contains_key("fingerprint") {
                if let Ok(e) = serde_json::from_value::<EdgeRef>(v.clone()) {
                    out.push((path.clone(), Found::Edge(e)));
                    return;
                }
                if let Ok(f) = serde_json::from_value::<FaceRef>(v.clone()) {
                    out.push((path.clone(), Found::Face(f)));
                    return;
                }
            }
            for (k, x) in o {
                let len = path.len();
                path.push('/');
                path.push_str(&k.replace('~', "~0").replace('/', "~1"));
                references(x, path, out);
                path.truncate(len);
            }
        }
        Value::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                let len = path.len();
                path.push_str(&format!("/{i}"));
                references(x, path, out);
                path.truncate(len);
            }
        }
        _ => {}
    }
}

fn edge_exists(scene: &Scene, e: &EdgeRef) -> bool {
    scene.bodies.iter().any(|b| b.edges.iter().any(|(names, _)| names.is_some_and(|[x, y]| [x, y] == e.faces || [y, x] == e.faces)))
}

fn face_exists(scene: &Scene, f: &FaceRef) -> bool {
    scene.bodies.iter().any(|b| b.faces.iter().any(|(name, _)| *name == Some(f.origin)))
}

/// Edges near where `e` was: an edge sharing one of its faces first, then by how far its middle
/// and its length are from the old ones.
fn edge_candidates(scene: &Scene, e: &EdgeRef) -> Vec<Candidate> {
    let mut all = Vec::new();
    for (bi, b) in scene.bodies.iter().enumerate() {
        for (i, (names, fp)) in b.edges.iter().enumerate() {
            let (Some([x, y]), Ok(index)) = (names, u32::try_from(i)) else { continue };
            let shares = e.faces.contains(x) || e.faces.contains(y);
            let distance = fp.mid.dist(e.fingerprint.mid) + (fp.length - e.fingerprint.length).abs();
            if let Some(r) = b.edge_ref(index).and_then(|r| serde_json::to_value(r).ok()) {
                all.push((!shares, Candidate { body: bi, index, reference: r, distance }));
            }
        }
    }
    nearest(all)
}

/// Faces near where `f` was: of the same surface sort, by how far the centroid is, how much the
/// direction turned (a right angle counts as 10 mm) and how much the area changed.
fn face_candidates(scene: &Scene, f: &FaceRef) -> Vec<Candidate> {
    let mut all = Vec::new();
    for (bi, b) in scene.bodies.iter().enumerate() {
        for (i, (name, info)) in b.faces.iter().enumerate() {
            let (Some(origin), Ok(index)) = (name, u32::try_from(i)) else { continue };
            let fp = Fingerprint::of(info);
            if fp.surface != f.fingerprint.surface {
                continue;
            }
            let turned = 1.0 - turn(fp.direction, f.fingerprint.direction);
            let area = (fp.area - f.fingerprint.area).abs() / f.fingerprint.area.abs().max(1e-9);
            let distance = fp.centroid.dist(f.fingerprint.centroid) + 10.0 * turned + area;
            if let Ok(r) = serde_json::to_value(FaceRef { origin: *origin, fingerprint: fp }) {
                all.push((false, Candidate { body: bi, index, reference: r, distance }));
            }
        }
    }
    nearest(all)
}

/// The cosine between two directions (1 when both are zero, as for spheres).
fn turn(a: Vec3, b: Vec3) -> f64 {
    let (la, lb) = (a.len(), b.len());
    if la < 1e-12 || lb < 1e-12 { 1.0 } else { a.dot(b) / (la * lb) }
}

fn nearest(mut all: Vec<(bool, Candidate)>) -> Vec<Candidate> {
    all.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.distance.total_cmp(&b.1.distance)));
    all.into_iter().take(CANDIDATES).map(|(_, c)| c).collect()
}
