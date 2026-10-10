//! Commands: every user action is a named command with JSON parameters, usable from the UI, the
//! CLI, scripts and the MCP server alike.
//!
//! Units in parameters: millimetres and radians. Entity, constraint and feature ids are the
//! integers shown by `sketch.info` and `model.tree`.

use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tenon_geom::{Vec2, tol};
use tenon_kernel::{Kernel, ShapeHandle};
use tenon_sketch::{Constraint, ConstraintId, EntityId, PointRef, Sketch, regions};

use crate::document::{
    AxisRef, AxisSel, Chamfer, ChamferSize, CircPattern, Coil, Combine, DRILL_POINT, DirectionRef, Draft, Extrude, ExtrudeExtent, FeatureKind,
    Fillet, Hole, HoleExtent, HoleType, Loft, Mirror, Operation, OriginAxis, OriginPlane, PlaneRef, RectPattern, RegionSel, Revolve, RevolveAngle,
    Rib, RibExtent, Shell, SketchCurves, Split, SplitKeep, Sweep, Thread, ThreadLength, WorkAxis, WorkPlane, WorkPoint, hole_centres, open_lines,
};
use crate::naming::{self, EdgeRef, FaceOrigin, FaceRef};
use crate::params::{ParamUnit, UserParam, ValuePath};
use crate::regen::{Regen, regenerate};
use crate::{Document, FeatureId};

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{0}")]
pub struct CmdError(pub String);

impl From<String> for CmdError {
    fn from(s: String) -> Self {
        CmdError(s)
    }
}
impl From<&str> for CmdError {
    fn from(s: &str) -> Self {
        CmdError(s.to_owned())
    }
}
impl From<tenon_sketch::SketchError> for CmdError {
    fn from(e: tenon_sketch::SketchError) -> Self {
        CmdError(e.to_string())
    }
}

pub type CmdResult = Result<Value, CmdError>;
pub type DocFn = fn(&mut Session, &Value) -> CmdResult;
pub type GeoFn = fn(&mut Session, &mut dyn Kernel, &Value) -> CmdResult;

/// How a command runs.
#[derive(Clone, Copy)]
pub enum Run {
    /// Edits or reads the document only.
    Doc(DocFn),
    /// Needs the geometry kernel.
    Geo(GeoFn),
}

/// A command.
#[derive(Clone, Copy)]
pub struct CommandSpec {
    pub id: &'static str,
    pub label: &'static str,
    /// Parameters, one line per parameter.
    pub help: &'static str,
    /// Edits the document as one undo step. (Undo, redo and opening a file change the document
    /// too, but add no step.)
    pub mutates: bool,
    pub run: Run,
}

impl std::fmt::Debug for CommandSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandSpec").field("id", &self.id).finish()
    }
}

/// Undo depth.
const MAX_UNDO: usize = 200;

/// A document with undo/redo and a cached regeneration.
pub struct Session {
    doc: Document,
    undo: Vec<Document>,
    redo: Vec<Document>,
    revision: u64,
    saved_revision: u64,
    regen: Option<(u64, Regen)>,
}

impl Default for Session {
    fn default() -> Self {
        Session::new(Document::default())
    }
}

impl Session {
    pub fn new(doc: Document) -> Self {
        Session { doc, undo: Vec::new(), redo: Vec::new(), revision: 1, saved_revision: 1, regen: None }
    }
    pub fn document(&self) -> &Document {
        &self.doc
    }
    /// Increases with every change to the document.
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision
    }
    pub fn mark_saved(&mut self) {
        self.saved_revision = self.revision;
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Replaces the document (e.g. after opening a file); undo history is cleared.
    pub fn replace_document(&mut self, mut doc: Document, k: Option<&mut dyn Kernel>) {
        self.release_regen(k);
        // Files from before parameters get their names now.
        doc.name_values();
        self.doc = doc;
        self.undo.clear();
        self.redo.clear();
        self.revision += 1;
        self.saved_revision = self.revision;
    }

    fn release_regen(&mut self, k: Option<&mut dyn Kernel>) {
        // Without a kernel the stale result stays cached (its revision no longer matches), and
        // the next `regen` releases its shapes; dropping it here would leak them in the kernel.
        if let Some(k) = k
            && let Some((_, mut r)) = self.regen.take()
        {
            r.release(k);
        }
    }

    /// Applies `f` to the document as one undoable step; on error nothing changes.
    pub fn edit<T>(&mut self, f: impl FnOnce(&mut Document) -> Result<T, CmdError>) -> Result<T, CmdError> {
        let before = self.doc.clone();
        // Equations follow every change, in the same undo step; a change they cannot follow
        // is refused.
        match f(&mut self.doc).and_then(|v| if self.doc != before { self.doc.sync_parameters().map(|()| v).map_err(CmdError) } else { Ok(v) }) {
            Ok(v) => {
                if self.doc != before {
                    self.undo.push(before);
                    if self.undo.len() > MAX_UNDO {
                        self.undo.remove(0);
                    }
                    self.redo.clear();
                    self.revision += 1;
                }
                Ok(v)
            }
            Err(e) => {
                self.doc = before;
                Err(e)
            }
        }
    }

    pub fn undo(&mut self) -> bool {
        match self.undo.pop() {
            Some(d) => {
                self.redo.push(std::mem::replace(&mut self.doc, d));
                self.revision += 1;
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.redo.pop() {
            Some(d) => {
                self.undo.push(std::mem::replace(&mut self.doc, d));
                self.revision += 1;
                true
            }
            None => false,
        }
    }

    /// The regeneration of the current revision (computed on demand, cached).
    pub fn regen(&mut self, k: &mut dyn Kernel) -> &Regen {
        if self.regen.as_ref().is_none_or(|(rev, _)| *rev != self.revision) {
            if let Some((_, mut old)) = self.regen.take() {
                old.release(k);
            }
            let r = regenerate(&self.doc, k);
            self.regen = Some((self.revision, r));
        }
        match &self.regen {
            Some((_, r)) => r,
            None => unreachable_regen(),
        }
    }

    /// Runs a command. `kernel` is required for geometry commands.
    pub fn run(&mut self, spec: &CommandSpec, params: &Value, kernel: Option<&mut dyn Kernel>) -> CmdResult {
        let params = if params.is_null() { &Value::Object(Default::default()) } else { params };
        if !params.is_object() {
            return Err("parameters must be a JSON object".into());
        }
        match spec.run {
            Run::Doc(f) => f(self, params),
            Run::Geo(f) => {
                let k = kernel.ok_or_else(|| CmdError(format!("{} needs the geometry kernel", spec.id)))?;
                f(self, k, params)
            }
        }
    }

    /// Runs a command of this crate's registry by id.
    pub fn exec(&mut self, id: &str, params: &Value, kernel: Option<&mut dyn Kernel>) -> CmdResult {
        let spec = find(id).ok_or_else(|| CmdError(format!("unknown command `{id}`")))?;
        self.run(spec, params, kernel)
    }
}

// The cached regeneration was set just above; this keeps `regen` free of panics.
fn unreachable_regen() -> &'static Regen {
    static EMPTY: std::sync::OnceLock<Regen> = std::sync::OnceLock::new();
    EMPTY.get_or_init(Regen::default)
}

// ---- parameter helpers ------------------------------------------------------------------------

fn field<'v>(p: &'v Value, key: &str) -> Result<&'v Value, CmdError> {
    p.get(key).ok_or_else(|| CmdError(format!("missing parameter `{key}`")))
}

fn num(p: &Value, key: &str) -> Result<f64, CmdError> {
    let v = field(p, key)?.as_f64().ok_or_else(|| CmdError(format!("`{key}` must be a number")))?;
    if tol::is_valid_coord(v) { Ok(v) } else { Err(CmdError(format!("`{key}` is out of range"))) }
}

fn opt_num(p: &Value, key: &str) -> Result<Option<f64>, CmdError> {
    if p.get(key).is_none_or(Value::is_null) { Ok(None) } else { num(p, key).map(Some) }
}

fn id_u32(p: &Value, key: &str) -> Result<u32, CmdError> {
    field(p, key)?.as_u64().and_then(|v| u32::try_from(v).ok()).ok_or_else(|| CmdError(format!("`{key}` must be an id")))
}

fn opt_id(p: &Value, key: &str) -> Result<Option<u32>, CmdError> {
    if p.get(key).is_none_or(Value::is_null) { Ok(None) } else { id_u32(p, key).map(Some) }
}

fn ids(p: &Value, key: &str) -> Result<Vec<EntityId>, CmdError> {
    let arr = field(p, key)?.as_array().ok_or_else(|| CmdError(format!("`{key}` must be a list of ids")))?;
    if arr.len() > 10_000 {
        return Err(CmdError(format!("`{key}` is too long")));
    }
    arr.iter()
        .map(|v| v.as_u64().and_then(|v| u32::try_from(v).ok()).map(EntityId).ok_or_else(|| CmdError(format!("`{key}` must be a list of ids"))))
        .collect()
}

fn parse<T: DeserializeOwned>(p: &Value, key: &str) -> Result<T, CmdError> {
    serde_json::from_value(field(p, key)?.clone()).map_err(|e| CmdError(format!("`{key}`: {e}")))
}

fn feature_id(p: &Value) -> Result<FeatureId, CmdError> {
    id_u32(p, "feature").map(FeatureId)
}

fn sketch_id(p: &Value) -> Result<FeatureId, CmdError> {
    id_u32(p, "sketch").map(FeatureId)
}

fn pos(p: &Value, x: &str, y: &str) -> Result<Vec2, CmdError> {
    Ok(Vec2::new(num(p, x)?, num(p, y)?))
}

/// A point parameter: an existing point id under `id_key`, else coordinates.
fn point_ref(p: &Value, id_key: &str, x: &str, y: &str) -> Result<PointRef, CmdError> {
    match opt_id(p, id_key)? {
        Some(id) => Ok(PointRef::Existing(EntityId(id))),
        None => Ok(PointRef::New(pos(p, x, y)?)),
    }
}

/// Edits the sketch of feature `sketch`.
fn on_sketch<T>(s: &mut Session, p: &Value, f: impl FnOnce(&mut Sketch) -> Result<T, CmdError>) -> Result<T, CmdError> {
    let id = sketch_id(p)?;
    s.edit(|doc| {
        let sk = doc.sketch_mut(id).ok_or_else(|| CmdError(format!("{id} is not a sketch")))?;
        f(sk)
    })
}

// ---- document commands ------------------------------------------------------------------------

fn document_rename(s: &mut Session, p: &Value) -> CmdResult {
    let name = field(p, "name")?.as_str().ok_or("`name` must be text")?.trim().to_owned();
    if name.is_empty() || name.len() > 200 {
        return Err("a name needs 1 to 200 characters".into());
    }
    s.edit(|d| {
        d.name = name;
        Ok(json!({}))
    })
}

/// `face: <face reference>` or `plane: "xy" | "yz" | "xz"` (default xy).
fn plane_param(p: &Value) -> Result<PlaneRef, CmdError> {
    if p.get("work_plane").is_some() {
        return Ok(PlaneRef::Work(FeatureId(id_u32(p, "work_plane")?)));
    }
    if let Some(face) = p.get("face") {
        return Ok(PlaneRef::Face(serde_json::from_value::<FaceRef>(face.clone()).map_err(|e| CmdError(format!("`face`: {e}")))?));
    }
    let name = p.get("plane").and_then(Value::as_str).unwrap_or("xy");
    Ok(PlaneRef::Origin(match name.to_ascii_lowercase().as_str() {
        "xy" => OriginPlane::XY,
        "yz" => OriginPlane::YZ,
        "xz" => OriginPlane::XZ,
        _ => return Err(format!("unknown plane `{name}` (xy, yz or xz)").into()),
    }))
}

fn origin_axis(name: &str) -> Result<OriginAxis, CmdError> {
    match name.to_ascii_lowercase().as_str() {
        "x" => Ok(OriginAxis::X),
        "y" => Ok(OriginAxis::Y),
        "z" => Ok(OriginAxis::Z),
        _ => Err(format!("unknown axis `{name}` (x, y or z)").into()),
    }
}

/// `{"work": id}`: a work feature.
fn work_id(v: &Value) -> Option<FeatureId> {
    v.get("work").and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok()).map(FeatureId)
}

/// A pattern direction: `"x" | "y" | "z"`, an edge reference, or `{"work": axis id}`.
fn direction_param(p: &Value, key: &str) -> Result<DirectionRef, CmdError> {
    let v = field(p, key)?;
    if let Some(id) = work_id(v) {
        return Ok(DirectionRef::Work(id));
    }
    match v {
        Value::String(a) => Ok(DirectionRef::Origin(origin_axis(a)?)),
        _ => Ok(DirectionRef::Edge(parse(p, key)?)),
    }
}

/// A turning axis: `"x" | "y" | "z"`, an edge reference, a face reference, or
/// `{"work": axis id}`.
fn axis_param(p: &Value, key: &str) -> Result<AxisSel, CmdError> {
    let v = field(p, key)?;
    if let Some(id) = work_id(v) {
        return Ok(AxisSel::Work(id));
    }
    match v {
        Value::String(a) => Ok(AxisSel::Origin(origin_axis(a)?)),
        Value::Object(o) if o.contains_key("faces") => Ok(AxisSel::Edge(parse(p, key)?)),
        Value::Object(o) if o.contains_key("origin") => Ok(AxisSel::Face(parse(p, key)?)),
        _ => Err(format!("`{key}` must be x, y, z, an edge reference, a face reference or {{\"work\": id}}").into()),
    }
}

/// A plane value: `"xy" | "yz" | "xz"`, a face reference, or `{"work": plane id}`.
fn plane_value(p: &Value, key: &str) -> Result<PlaneRef, CmdError> {
    let v = field(p, key)?;
    if let Some(id) = work_id(v) {
        return Ok(PlaneRef::Work(id));
    }
    match v {
        Value::String(s) => match s.to_ascii_lowercase().as_str() {
            "xy" => Ok(PlaneRef::Origin(OriginPlane::XY)),
            "yz" => Ok(PlaneRef::Origin(OriginPlane::YZ)),
            "xz" => Ok(PlaneRef::Origin(OriginPlane::XZ)),
            _ => Err(format!("unknown plane `{s}` (xy, yz or xz)").into()),
        },
        _ => Ok(PlaneRef::Face(parse(p, key)?)),
    }
}

/// Work features must refer to work features of the right kind.
fn check_work_refs(s: &Session, kind: &FeatureKind) -> Result<(), CmdError> {
    let doc = s.document();
    let is = |id: FeatureId, want: fn(&FeatureKind) -> bool, what: &str| -> Result<(), CmdError> {
        match doc.feature(id) {
            Some(f) if want(&f.kind) => Ok(()),
            Some(f) => Err(format!("{} is not a {what}", f.name).into()),
            None => Err(format!("{id} does not exist").into()),
        }
    };
    let plane = |r: &PlaneRef| match r {
        PlaneRef::Work(id) => is(*id, |k| matches!(k, FeatureKind::WorkPlane(_)), "work plane"),
        _ => Ok(()),
    };
    let axis = |a: &AxisSel| match a {
        AxisSel::Work(id) => is(*id, |k| matches!(k, FeatureKind::WorkAxis(_)), "work axis"),
        _ => Ok(()),
    };
    match kind {
        FeatureKind::WorkPlane(WorkPlane::Offset { base, .. }) => plane(base),
        FeatureKind::WorkPlane(WorkPlane::Angle { base, axis: a, .. }) => plane(base).and(axis(a)),
        FeatureKind::WorkPlane(WorkPlane::Midplane { a, b }) => plane(a).and(plane(b)),
        FeatureKind::WorkAxis(WorkAxis::Along { axis: a }) => axis(a),
        FeatureKind::WorkAxis(WorkAxis::Planes { a, b }) => plane(a).and(plane(b)),
        FeatureKind::WorkPoint(WorkPoint::Intersection { axis: a, plane: p }) => axis(a).and(plane(p)),
        FeatureKind::Mirror(m) => plane(&m.plane),
        FeatureKind::PatternCircular(c) => axis(&c.axis),
        FeatureKind::PatternRect(r) => std::iter::once(&r.dir1).chain(r.dir2.as_ref()).try_for_each(|d| match d {
            DirectionRef::Work(id) => is(*id, |k| matches!(k, FeatureKind::WorkAxis(_)), "work axis"),
            _ => Ok(()),
        }),
        FeatureKind::Sketch { plane: p, .. } => plane(p),
        _ => Ok(()),
    }
}

fn work_plane(s: &mut Session, p: &Value) -> CmdResult {
    let def = match p.get("by").and_then(Value::as_str).unwrap_or("offset") {
        "offset" => WorkPlane::Offset { base: plane_value(p, "base")?, distance: num(p, "distance")? },
        "angle" => WorkPlane::Angle { base: plane_value(p, "base")?, axis: axis_param(p, "axis")?, angle: num(p, "angle")? },
        "midplane" => WorkPlane::Midplane { a: plane_value(p, "a")?, b: plane_value(p, "b")? },
        other => return Err(format!("unknown work plane `{other}` (offset, angle, midplane)").into()),
    };
    let kind = FeatureKind::WorkPlane(def);
    check_work_refs(s, &kind)?;
    add_feature(s, kind, p)
}

fn work_axis(s: &mut Session, p: &Value) -> CmdResult {
    let def = if p.get("a").is_some() {
        WorkAxis::Planes { a: plane_value(p, "a")?, b: plane_value(p, "b")? }
    } else {
        WorkAxis::Along { axis: axis_param(p, "axis")? }
    };
    let kind = FeatureKind::WorkAxis(def);
    check_work_refs(s, &kind)?;
    add_feature(s, kind, p)
}

fn work_point(s: &mut Session, p: &Value) -> CmdResult {
    let def = if p.get("edge").is_some() {
        WorkPoint::Center { edge: parse(p, "edge")? }
    } else {
        WorkPoint::Intersection { axis: axis_param(p, "axis")?, plane: plane_value(p, "plane")? }
    };
    let kind = FeatureKind::WorkPoint(def);
    check_work_refs(s, &kind)?;
    add_feature(s, kind, p)
}

fn feature_list(p: &Value) -> Result<Vec<FeatureId>, CmdError> {
    Ok(ids(p, "features")?.into_iter().map(|e| FeatureId(e.0)).collect())
}

fn count(p: &Value, key: &str, default: Option<u32>) -> Result<u32, CmdError> {
    match (p.get(key), default) {
        (None | Some(Value::Null), Some(d)) => Ok(d),
        _ => id_u32(p, key).map_err(|_| CmdError(format!("`{key}` must be a whole number"))),
    }
}

/// Adds a pattern or mirror after checking what it copies.
fn add_copy(s: &mut Session, kind: FeatureKind, p: &Value) -> CmdResult {
    s.document().check_copies(&kind).map_err(CmdError)?;
    check_work_refs(s, &kind)?;
    add_feature(s, kind, p)
}

fn model_pattern_rect(s: &mut Session, p: &Value) -> CmdResult {
    let flag = |k: &str| p.get(k).and_then(Value::as_bool).unwrap_or(false);
    let dir2 = if p.get("direction2").is_some_and(|v| !v.is_null()) { Some(direction_param(p, "direction2")?) } else { None };
    let pat = RectPattern {
        features: feature_list(p)?,
        dir1: direction_param(p, "direction")?,
        count1: count(p, "count", None)?,
        spacing1: num(p, "spacing")?,
        reverse1: flag("reverse"),
        count2: count(p, "count2", Some(1))?,
        spacing2: opt_num(p, "spacing2")?.unwrap_or(0.0),
        reverse2: flag("reverse2"),
        dir2,
    };
    pat.check().map_err(CmdError)?;
    add_copy(s, FeatureKind::PatternRect(pat), p)
}

fn model_pattern_circular(s: &mut Session, p: &Value) -> CmdResult {
    let pat = CircPattern {
        features: feature_list(p)?,
        axis: axis_param(p, "axis")?,
        count: count(p, "count", None)?,
        angle: opt_num(p, "angle")?.unwrap_or(std::f64::consts::TAU),
        reverse: p.get("reverse").and_then(Value::as_bool).unwrap_or(false),
    };
    pat.check().map_err(CmdError)?;
    add_copy(s, FeatureKind::PatternCircular(pat), p)
}

fn model_mirror(s: &mut Session, p: &Value) -> CmdResult {
    let features = feature_list(p)?;
    if features.is_empty() {
        return Err("choose at least one feature to mirror".into());
    }
    add_copy(s, FeatureKind::Mirror(Mirror { features, plane: plane_param(p)? }), p)
}

fn sketch_create(s: &mut Session, p: &Value) -> CmdResult {
    let plane = plane_param(p)?;
    if let PlaneRef::Work(id) = plane
        && !s.document().feature(id).is_some_and(|f| matches!(f.kind, FeatureKind::WorkPlane(_)))
    {
        return Err(format!("{id} is not a work plane").into());
    }
    // The part origin projected into the sketch: a fixed construction point at (0, 0) that
    // geometry can snap and be constrained to.
    let mut sketch = Sketch::new();
    let origin = if p.get("project_origin").and_then(Value::as_bool).unwrap_or(true) {
        let o = sketch.add_point(Vec2::new(0.0, 0.0))?;
        sketch.set_construction(o, true)?;
        sketch.add_constraint(Constraint::Fix { point: o })?;
        Some(o.0)
    } else {
        None
    };
    let id = s.edit(|d| d.add(FeatureKind::Sketch { plane, sketch }).map_err(CmdError))?;
    let name = s.document().feature(id).map(|f| f.name.clone()).unwrap_or_default();
    Ok(json!({ "feature": id.0, "name": name, "origin": origin }))
}

fn sketch_point(s: &mut Session, p: &Value) -> CmdResult {
    let at = pos(p, "x", "y")?;
    let id = on_sketch(s, p, |sk| Ok(sk.add_point(at)?))?;
    Ok(json!({ "point": id.0 }))
}

fn sketch_line(s: &mut Session, p: &Value) -> CmdResult {
    let (a, b) = (point_ref(p, "start", "x1", "y1")?, point_ref(p, "end", "x2", "y2")?);
    let (line, start, end) = on_sketch(s, p, |sk| {
        let line = sk.add_line(a, b)?;
        match sk.geometry(line) {
            Some(tenon_sketch::Geometry::Line { start, end }) => Ok((line, *start, *end)),
            _ => Err("internal: not a line".into()),
        }
    })?;
    Ok(json!({ "line": line.0, "start": start.0, "end": end.0 }))
}

fn sketch_circle(s: &mut Session, p: &Value) -> CmdResult {
    let c = point_ref(p, "center", "cx", "cy")?;
    let r = num(p, "r")?;
    let id = on_sketch(s, p, |sk| Ok(sk.add_circle(c, r)?))?;
    Ok(json!({ "circle": id.0 }))
}

fn sketch_arc(s: &mut Session, p: &Value) -> CmdResult {
    let (c, a, b) = (point_ref(p, "center", "cx", "cy")?, point_ref(p, "start", "x1", "y1")?, point_ref(p, "end", "x2", "y2")?);
    let id = on_sketch(s, p, |sk| Ok(sk.add_arc(c, a, b)?))?;
    Ok(json!({ "arc": id.0 }))
}

fn sketch_arc3(s: &mut Session, p: &Value) -> CmdResult {
    let (a, m, b) = (pos(p, "x1", "y1")?, pos(p, "x2", "y2")?, pos(p, "x3", "y3")?);
    let id = on_sketch(s, p, |sk| Ok(sk.add_arc_three_point(a, m, b)?))?;
    Ok(json!({ "arc": id.0 }))
}

fn sketch_rectangle(s: &mut Session, p: &Value) -> CmdResult {
    let (a, b) = (pos(p, "x1", "y1")?, pos(p, "x2", "y2")?);
    let lines = on_sketch(s, p, |sk| Ok(sk.add_rectangle(a, b)?))?;
    // Each line's start point: the corners, in order (the first is at x1, y1).
    let sk = s.document().sketch(sketch_id(p)?);
    let corners: Vec<u32> = lines
        .iter()
        .filter_map(|l| match sk?.geometry(*l)? {
            tenon_sketch::Geometry::Line { start, .. } => Some(start.0),
            _ => None,
        })
        .collect();
    Ok(json!({ "lines": lines.iter().map(|l| l.0).collect::<Vec<_>>(), "corners": corners }))
}

fn sketch_polygon(s: &mut Session, p: &Value) -> CmdResult {
    let (c, v) = (pos(p, "cx", "cy")?, pos(p, "x", "y")?);
    let sides = id_u32(p, "sides")?;
    let lines = on_sketch(s, p, |sk| Ok(sk.add_polygon(c, v, sides)?))?;
    Ok(json!({ "lines": lines.iter().map(|l| l.0).collect::<Vec<_>>() }))
}

fn sketch_spline(s: &mut Session, p: &Value) -> CmdResult {
    let pts: Vec<[f64; 2]> = parse(p, "points")?;
    let degree = opt_id(p, "degree")?.unwrap_or(3);
    let refs: Vec<PointRef> = pts.iter().map(|q| PointRef::New(Vec2::new(q[0], q[1]))).collect();
    let id = on_sketch(s, p, |sk| Ok(sk.add_spline(&refs, degree)?))?;
    Ok(json!({ "spline": id.0 }))
}

fn sketch_constrain(s: &mut Session, p: &Value) -> CmdResult {
    let c: Constraint = parse(p, "constraint")?;
    let sketch = sketch_id(p)?;
    // A dimension may come with an equation that drives it (`d0 / 2`).
    let eq = param_value(p, "equation")?.filter(|e| e.trim().parse::<f64>().is_err());
    let id = s.edit(|d| {
        let sk = d.sketch_mut(sketch).ok_or_else(|| CmdError(format!("{sketch} is not a sketch")))?;
        let id = sk.add_constraint(c)?;
        if let Some(eq) = &eq {
            d.name_values();
            let name = d.name_of(&ValuePath::Dimension { sketch, constraint: id }).ok_or("only a dimension can have an equation")?.to_owned();
            d.set_equation(&name, Some(eq)).map_err(CmdError)?;
        }
        Ok(id)
    })?;
    let name = s.document().name_of(&ValuePath::Dimension { sketch, constraint: id }).map(str::to_owned);
    Ok(json!({ "constraint": id.0, "name": name }))
}

/// Sets a dimension's value (clearing its equation), or with `equation` drives it by one.
fn sketch_set_dimension(s: &mut Session, p: &Value) -> CmdResult {
    let (sketch, c) = (sketch_id(p)?, ConstraintId(id_u32(p, "constraint")?));
    let path = ValuePath::Dimension { sketch, constraint: c };
    match param_value(p, "equation")? {
        Some(eq) if eq.trim().parse::<f64>().is_err() => s.edit(|d| {
            d.sketch(sketch)
                .and_then(|sk| sk.constraint(c))
                .filter(|x| x.is_dimensional())
                .ok_or_else(|| CmdError(format!("{c} is not a dimension of {sketch}")))?;
            d.name_values();
            let name = d.name_of(&path).ok_or("the dimension has no name")?.to_owned();
            d.set_equation(&name, Some(&eq)).map_err(CmdError)
        })?,
        given => {
            let v = match given {
                Some(eq) => eq.trim().parse::<f64>().map_err(|_| CmdError("not a number".into()))?,
                None => num(p, "value")?,
            };
            s.edit(|d| {
                let sk = d.sketch_mut(sketch).ok_or_else(|| CmdError(format!("{sketch} is not a sketch")))?;
                sk.set_dimension(c, v)?;
                d.clear_equation_at(&path);
                Ok(())
            })?;
        }
    }
    let name = s.document().name_of(&path).map(str::to_owned);
    Ok(json!({ "name": name }))
}

fn sketch_remove_constraint(s: &mut Session, p: &Value) -> CmdResult {
    let c = ConstraintId(id_u32(p, "constraint")?);
    on_sketch(s, p, |sk| Ok(sk.remove_constraint(c).map(|_| ())?))?;
    Ok(json!({}))
}

fn sketch_drag(s: &mut Session, p: &Value) -> CmdResult {
    let (pt, to) = (EntityId(id_u32(p, "point")?), pos(p, "x", "y")?);
    on_sketch(s, p, |sk| Ok(sk.drag(pt, to)?))?;
    Ok(json!({}))
}

fn sketch_delete(s: &mut Session, p: &Value) -> CmdResult {
    let list = ids(p, "entities")?;
    let gone = on_sketch(s, p, |sk| {
        if let Some(missing) = list.iter().find(|e| sk.entity(**e).is_none()) {
            return Err(CmdError(format!("no entity {missing}")));
        }
        Ok(sk.delete(&list))
    })?;
    Ok(json!({ "deleted": gone.iter().map(|e| e.0).collect::<Vec<_>>() }))
}

fn sketch_construction(s: &mut Session, p: &Value) -> CmdResult {
    let e = EntityId(id_u32(p, "entity")?);
    let on = p.get("on").and_then(Value::as_bool).unwrap_or(true);
    on_sketch(s, p, |sk| Ok(sk.set_construction(e, on)?))?;
    Ok(json!({}))
}

fn sketch_fillet(s: &mut Session, p: &Value) -> CmdResult {
    let (pt, r) = (EntityId(id_u32(p, "point")?), num(p, "radius")?);
    let arc = on_sketch(s, p, |sk| Ok(sk.fillet(pt, r)?))?;
    Ok(json!({ "arc": arc.0 }))
}

fn sketch_trim(s: &mut Session, p: &Value) -> CmdResult {
    let (c, at) = (EntityId(id_u32(p, "curve")?), pos(p, "x", "y")?);
    on_sketch(s, p, |sk| Ok(sk.trim(c, at)?))?;
    Ok(json!({}))
}

fn sketch_offset(s: &mut Session, p: &Value) -> CmdResult {
    let (list, d) = (ids(p, "curves")?, num(p, "distance")?);
    let new = on_sketch(s, p, |sk| Ok(sk.offset(&list, d)?))?;
    Ok(json!({ "curves": new.iter().map(|e| e.0).collect::<Vec<_>>() }))
}

fn sketch_mirror(s: &mut Session, p: &Value) -> CmdResult {
    let (list, axis) = (ids(p, "entities")?, EntityId(id_u32(p, "axis")?));
    let new = on_sketch(s, p, |sk| Ok(sk.mirror(&list, axis)?))?;
    Ok(json!({ "entities": new.iter().map(|e| e.0).collect::<Vec<_>>() }))
}

/// Entities, constraints, degrees of freedom and closed regions of a sketch.
pub fn sketch_info_value(sk: &Sketch) -> Value {
    let dof = sk.dof();
    let entities: Vec<Value> = sk
        .entities()
        .map(|(id, e)| {
            let mut v = serde_json::to_value(&e.geometry).unwrap_or(Value::Null);
            if let Some(o) = v.as_object_mut() {
                o.insert("id".into(), json!(id.0));
                o.insert("construction".into(), json!(e.construction));
                o.insert("fully_constrained".into(), json!(dof.as_ref().is_ok_and(|d| d.fully_constrained.contains(&id))));
            }
            v
        })
        .collect();
    let constraints: Vec<Value> = sk
        .constraints()
        .map(|(id, c)| {
            let mut v = serde_json::to_value(c).unwrap_or(Value::Null);
            if let Some(o) = v.as_object_mut() {
                o.insert("id".into(), json!(id.0));
            }
            v
        })
        .collect();
    let regions: Vec<Value> = regions(sk)
        .iter()
        .map(|r| json!({ "key": r.key.iter().map(|e| e.0).collect::<Vec<_>>(), "area": r.area, "depth": r.depth, "holes": r.holes.len() }))
        .collect();
    json!({
        "entities": entities,
        "constraints": constraints,
        "dof": dof.as_ref().map(|d| json!(d.dof)).unwrap_or(Value::Null),
        "regions": regions,
    })
}

fn sketch_info(s: &mut Session, p: &Value) -> CmdResult {
    let id = sketch_id(p)?;
    let sk = s.document().sketch(id).ok_or_else(|| CmdError(format!("{id} is not a sketch")))?;
    Ok(sketch_info_value(sk))
}

fn operation(p: &Value) -> Result<Operation, CmdError> {
    match p.get("operation").and_then(Value::as_str).unwrap_or("join") {
        "join" => Ok(Operation::Join),
        "cut" => Ok(Operation::Cut),
        "new_body" => Ok(Operation::NewBody),
        "intersect" => Ok(Operation::Intersect),
        other => Err(CmdError(format!("unknown operation `{other}` (join, cut, new_body, intersect)"))),
    }
}

fn region_sel(p: &Value) -> Result<RegionSel, CmdError> {
    match p.get("regions") {
        None | Some(Value::Null) => Ok(RegionSel::Default),
        Some(_) => {
            let keys: Vec<Vec<u32>> = parse(p, "regions")?;
            Ok(RegionSel::Keys(keys.into_iter().map(|k| k.into_iter().map(EntityId).collect()).collect()))
        }
    }
}

/// Adds a feature, with `equations` (value field: equation) from the parameters if given, in one
/// undo step.
fn add_feature(s: &mut Session, kind: FeatureKind, p: &Value) -> CmdResult {
    let equations = equations_param(p)?;
    let id = s.edit(|d| {
        let id = d.add(kind).map_err(CmdError)?;
        apply_feature_equations(d, id, &[], &equations)?;
        Ok(id)
    })?;
    let name = s.document().feature(id).map(|f| f.name.clone()).unwrap_or_default();
    Ok(json!({ "feature": id.0, "name": name }))
}

fn model_extrude(s: &mut Session, p: &Value) -> CmdResult {
    let sketch = sketch_id(p)?;
    let extent = if p.get("through_all").and_then(Value::as_bool).unwrap_or(false) {
        ExtrudeExtent::ThroughAll
    } else if let Some(d) = opt_num(p, "symmetric")? {
        ExtrudeExtent::Symmetric(d)
    } else if let Some(back) = opt_num(p, "backward")? {
        ExtrudeExtent::TwoSided { forward: num(p, "distance")?, backward: back }
    } else {
        ExtrudeExtent::Distance(num(p, "distance")?)
    };
    let reverse = p.get("reverse").and_then(Value::as_bool).unwrap_or(false);
    let extrude =
        Extrude { sketch, regions: region_sel(p)?, extent, reverse, operation: operation(p)?, taper: opt_num(p, "taper")?.filter(|t| *t != 0.0) };
    extrude.check().map_err(CmdError)?;
    add_feature(s, FeatureKind::Extrude(extrude), p)
}

fn model_revolve(s: &mut Session, p: &Value) -> CmdResult {
    let sketch = sketch_id(p)?;
    let axis = axis_ref(s, p)?;
    let angle = match opt_num(p, "angle")? {
        None => RevolveAngle::Full,
        Some(a) if p.get("symmetric").and_then(Value::as_bool).unwrap_or(false) => RevolveAngle::Symmetric(a),
        Some(a) => RevolveAngle::Angle(a),
    };
    add_feature(s, FeatureKind::Revolve(Revolve { sketch, regions: region_sel(p)?, axis, angle, operation: operation(p)? }), p)
}

fn edge_refs(p: &Value) -> Result<Vec<EdgeRef>, CmdError> {
    let v: Vec<EdgeRef> = parse(p, "edges")?;
    if v.is_empty() || v.len() > 10_000 {
        return Err("`edges` needs 1 to 10000 edge references (from model.edge_ref)".into());
    }
    Ok(v)
}

fn model_fillet(s: &mut Session, p: &Value) -> CmdResult {
    add_feature(s, FeatureKind::Fillet(Fillet { edges: edge_refs(p)?, radius: num(p, "radius")? }), p)
}

fn model_chamfer(s: &mut Session, p: &Value) -> CmdResult {
    let edges = edge_refs(p)?;
    let d = num(p, "distance")?;
    let size = match (opt_num(p, "distance2")?, opt_num(p, "angle")?) {
        (None, None) => ChamferSize::Equal(d),
        (Some(d2), None) => ChamferSize::TwoDistances { d1: d, d2, reference: parse(p, "reference")? },
        (None, Some(angle)) => ChamferSize::DistanceAngle { distance: d, angle, reference: parse(p, "reference")? },
        (Some(_), Some(_)) => return Err("give `distance2` or `angle`, not both".into()),
    };
    add_feature(s, FeatureKind::Chamfer(Chamfer { edges, size }), p)
}

fn model_shell(s: &mut Session, p: &Value) -> CmdResult {
    let remove: Vec<FaceRef> = if p.get("remove").is_some() { parse(p, "remove")? } else { Vec::new() };
    let outside = p.get("outside").and_then(Value::as_bool).unwrap_or(false);
    add_feature(s, FeatureKind::Shell(Shell { remove, thickness: num(p, "thickness")?, outside }), p)
}

fn model_hole(s: &mut Session, p: &Value) -> CmdResult {
    let sketch = sketch_id(p)?;
    let sk = s.document().sketch(sketch).ok_or_else(|| CmdError(format!("{sketch} is not a sketch")))?;
    let points = if p.get("points").is_some() { ids(p, "points")? } else { hole_centres(sk) };
    if let Some(bad) = points.iter().find(|e| sk.point(**e).is_none()) {
        return Err(format!("{} is not a point of {sketch}", bad.0).into());
    }
    let diameter = num(p, "diameter")?;
    let kind = match p.get("type").and_then(Value::as_str).unwrap_or("simple") {
        "simple" => HoleType::Simple,
        "counterbore" => HoleType::Counterbore { diameter: num(p, "counterbore_diameter")?, depth: num(p, "counterbore_depth")? },
        "countersink" => HoleType::Countersink {
            diameter: num(p, "countersink_diameter")?,
            angle: opt_num(p, "countersink_angle")?.unwrap_or(std::f64::consts::FRAC_PI_2),
        },
        other => return Err(format!("unknown hole type `{other}` (simple, counterbore, countersink)").into()),
    };
    let extent =
        if p.get("through_all").and_then(Value::as_bool).unwrap_or(false) { HoleExtent::ThroughAll } else { HoleExtent::Distance(num(p, "depth")?) };
    let tip_angle =
        if p.get("flat_bottom").and_then(Value::as_bool).unwrap_or(false) { None } else { Some(opt_num(p, "tip_angle")?.unwrap_or(DRILL_POINT)) };
    let reverse = p.get("reverse").and_then(Value::as_bool).unwrap_or(false);
    let hole = Hole { sketch, points, diameter, kind, extent, tip_angle, reverse };
    hole.check().map_err(CmdError)?;
    add_feature(s, FeatureKind::Hole(hole), p)
}

/// `equations: {field pointer: equation}` for a feature's values.
fn equations_param(p: &Value) -> Result<Vec<(String, String)>, CmdError> {
    match p.get("equations") {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Object(o)) => o
            .iter()
            .map(|(k, v)| match v {
                Value::String(e) => Ok((k.clone(), e.clone())),
                Value::Number(n) => Ok((k.clone(), n.to_string())),
                _ => Err(CmdError(format!("the equation for `{k}` must be text"))),
            })
            .collect(),
        Some(_) => Err("`equations` must map value fields to equations".into()),
    }
}

/// Values of a feature that changed directly lose their equations; `equations` set new ones.
fn apply_feature_equations(d: &mut Document, id: FeatureId, before: &[(ValuePath, f64)], equations: &[(String, String)]) -> Result<(), CmdError> {
    for (path, old) in before {
        if d.value_at(path).is_some_and(|(v, _)| (v - old).abs() > 1e-12 * (1.0 + v.abs())) {
            d.clear_equation_at(path);
        }
    }
    d.name_values();
    for (field, eq) in equations {
        let path = ValuePath::Feature { feature: id, field: field.clone() };
        let name = d.name_of(&path).ok_or_else(|| CmdError(format!("`{field}` is not a value of {id}")))?.to_owned();
        let plain = eq.trim().parse::<f64>().is_ok();
        d.set_equation(&name, if plain { None } else { Some(eq) }).map_err(CmdError)?;
        if plain {
            d.set_value_at(&path, eq.trim().parse::<f64>().unwrap_or_default()).map_err(CmdError)?;
        }
    }
    Ok(())
}

fn feature_values(d: &Document, id: FeatureId) -> Vec<(ValuePath, f64)> {
    d.value_paths()
        .into_iter()
        .filter(|(p, _)| matches!(p, ValuePath::Feature { feature, .. } if *feature == id))
        .filter_map(|(p, _)| d.value_at(&p).map(|(v, _)| (p, v)))
        .collect()
}

/// Adds a feature from its definition, with equations for its values, in one step.
fn feature_add(s: &mut Session, p: &Value) -> CmdResult {
    let kind: FeatureKind = parse(p, "kind")?;
    if matches!(kind, FeatureKind::Sketch { .. }) {
        return Err("use sketch.create for sketches".into());
    }
    let equations = equations_param(p)?;
    s.document().check_copies(&kind).map_err(CmdError)?;
    check_work_refs(s, &kind)?;
    let id = s.edit(|d| {
        let id = d.add(kind).map_err(CmdError)?;
        d.validate().map_err(CmdError)?;
        apply_feature_equations(d, id, &[], &equations)?;
        Ok(id)
    })?;
    let name = s.document().feature(id).map(|f| f.name.clone()).unwrap_or_default();
    Ok(json!({ "feature": id.0, "name": name }))
}

fn model_rib(s: &mut Session, p: &Value) -> CmdResult {
    let sketch = sketch_id(p)?;
    let sk = s.document().sketch(sketch).ok_or_else(|| CmdError(format!("{sketch} is not a sketch")))?;
    let lines = if p.get("lines").is_some() { ids(p, "lines")? } else { open_lines(sk) };
    if let Some(bad) = lines.iter().find(|l| !sk.is_line(**l)) {
        return Err(format!("{} is not a line of {sketch}", bad.0).into());
    }
    let extent = match opt_num(p, "distance")? {
        Some(d) => RibExtent::Distance(d),
        None => RibExtent::ToNext,
    };
    let rib = Rib { sketch, lines, thickness: num(p, "thickness")?, extent, flip: p.get("flip").and_then(Value::as_bool).unwrap_or(false) };
    rib.check().map_err(CmdError)?;
    add_feature(s, FeatureKind::Rib(rib), p)
}

/// An axis for revolves and coils: "x" | "y" | "z", a line id of the sketch, or {"work": id}.
fn axis_ref(s: &Session, p: &Value) -> Result<AxisRef, CmdError> {
    Ok(match field(p, "axis")? {
        Value::String(a) => AxisRef::Origin(match a.to_ascii_lowercase().as_str() {
            "x" => OriginAxis::X,
            "y" => OriginAxis::Y,
            "z" => OriginAxis::Z,
            _ => return Err(format!("unknown axis `{a}` (x, y, z or a sketch line id)").into()),
        }),
        v if work_id(v).is_some() => {
            let id = work_id(v).ok_or("bad work axis")?;
            check_work_refs(s, &FeatureKind::WorkAxis(WorkAxis::Along { axis: AxisSel::Work(id) }))?;
            AxisRef::Work(id)
        }
        _ => AxisRef::SketchLine(EntityId(id_u32(p, "axis")?)),
    })
}

fn model_sweep(s: &mut Session, p: &Value) -> CmdResult {
    let sketch = sketch_id(p)?;
    let path_sketch = FeatureId(id_u32(p, "path_sketch")?);
    let sk = s.document().sketch(path_sketch).ok_or_else(|| CmdError(format!("{path_sketch} is not a sketch")))?;
    // By default, every line and arc of the path's sketch that is not construction.
    let curves = if p.get("path").is_some() {
        ids(p, "path")?
    } else {
        sk.entities()
            .filter(|(_, e)| !e.construction && matches!(e.geometry, tenon_sketch::Geometry::Line { .. } | tenon_sketch::Geometry::Arc { .. }))
            .map(|(id, _)| id)
            .collect()
    };
    let sweep = Sweep {
        sketch,
        regions: region_sel(p)?,
        path: SketchCurves { sketch: path_sketch, curves },
        fixed: p.get("fixed").and_then(Value::as_bool).unwrap_or(false),
        operation: operation(p)?,
    };
    sweep.check().map_err(CmdError)?;
    add_feature(s, FeatureKind::Sweep(sweep), p)
}

fn model_coil(s: &mut Session, p: &Value) -> CmdResult {
    let coil = Coil {
        sketch: sketch_id(p)?,
        regions: region_sel(p)?,
        axis: axis_ref(s, p)?,
        pitch: num(p, "pitch")?,
        turns: num(p, "turns")?,
        left: p.get("left").and_then(Value::as_bool).unwrap_or(false),
        operation: operation(p)?,
    };
    coil.check().map_err(CmdError)?;
    add_feature(s, FeatureKind::Coil(coil), p)
}

fn model_loft(s: &mut Session, p: &Value) -> CmdResult {
    let loft = Loft {
        sections: ids(p, "sections")?.into_iter().map(|e| FeatureId(e.0)).collect(),
        ruled: p.get("ruled").and_then(Value::as_bool).unwrap_or(false),
        operation: operation(p)?,
    };
    loft.check().map_err(CmdError)?;
    if let Some(bad) = loft.sections.iter().find(|f| s.document().sketch(**f).is_none()) {
        return Err(format!("{bad} is not a sketch").into());
    }
    add_feature(s, FeatureKind::Loft(loft), p)
}

/// A plane that must be named: plane ("xy" | "yz" | "xz"), face (a planar face reference) or
/// work_plane (a work plane id).
fn plane_named(s: &Session, p: &Value) -> Result<PlaneRef, CmdError> {
    if !["plane", "face", "work_plane"].iter().any(|k| p.get(*k).is_some()) {
        return Err("name the plane: plane (xy, yz or xz), face (a planar face reference) or work_plane (a work plane id)".into());
    }
    let plane = plane_param(p)?;
    if let PlaneRef::Work(id) = plane
        && !s.document().feature(id).is_some_and(|f| matches!(f.kind, FeatureKind::WorkPlane(_)))
    {
        return Err(format!("{id} is not a work plane").into());
    }
    Ok(plane)
}

fn model_draft(s: &mut Session, p: &Value) -> CmdResult {
    let draft = Draft {
        faces: parse(p, "faces")?,
        plane: plane_named(s, p)?,
        angle: num(p, "angle")?,
        reverse: p.get("reverse").and_then(Value::as_bool).unwrap_or(false),
    };
    draft.check().map_err(CmdError)?;
    add_feature(s, FeatureKind::Draft(draft), p)
}

fn model_split(s: &mut Session, p: &Value) -> CmdResult {
    let keep = match p.get("keep").and_then(Value::as_str).unwrap_or("both") {
        "both" => SplitKeep::Both,
        "front" => SplitKeep::Front,
        "back" => SplitKeep::Back,
        other => return Err(format!("unknown keep `{other}` (both, front, back)").into()),
    };
    let body = if p.get("body").is_some() { Some(parse(p, "body")?) } else { None };
    add_feature(s, FeatureKind::Split(Split { plane: plane_named(s, p)?, keep, body }), p)
}

fn model_combine(s: &mut Session, p: &Value) -> CmdResult {
    let combine = Combine {
        base: parse(p, "base")?,
        tools: parse(p, "tools")?,
        operation: operation(p)?,
        keep_tools: p.get("keep_tools").and_then(Value::as_bool).unwrap_or(false),
    };
    combine.check().map_err(CmdError)?;
    add_feature(s, FeatureKind::Combine(combine), p)
}

fn model_thread(s: &mut Session, p: &Value) -> CmdResult {
    let length = match p.get("length") {
        None | Some(Value::Null) => ThreadLength::Full,
        Some(_) => ThreadLength::Distance(num(p, "length")?),
    };
    let thread = Thread {
        face: parse(p, "face")?,
        pitch: if p.get("pitch").is_some_and(|v| !v.is_null()) { Some(num(p, "pitch")?) } else { None },
        designation: p.get("designation").and_then(Value::as_str).map(str::to_owned),
        length,
        reverse: p.get("reverse").and_then(Value::as_bool).unwrap_or(false),
        left: p.get("left").and_then(Value::as_bool).unwrap_or(false),
        modelled: p.get("modelled").and_then(Value::as_bool).unwrap_or(false),
    };
    thread.check().map_err(CmdError)?;
    add_feature(s, FeatureKind::Thread(thread), p)
}

/// The threads on the part as it regenerates.
fn model_threads(s: &mut Session, k: &mut dyn Kernel, _p: &Value) -> CmdResult {
    let doc = s.document().clone();
    let threads: Vec<Value> = s
        .regen(k)
        .threads
        .iter()
        .map(|t| {
            json!({
                "feature": t.feature.0,
                "name": doc.feature(t.feature).map(|f| f.name.clone()),
                "designation": t.designation,
                "pitch": t.pitch,
                "diameter": t.diameter,
                "internal": t.internal,
                "length": t.length,
                "left": t.left,
                "modelled": t.modelled,
                "start": [t.start.x, t.start.y, t.start.z],
                "direction": [t.direction.x, t.direction.y, t.direction.z],
            })
        })
        .collect();
    Ok(json!({ "threads": threads }))
}

fn feature_update(s: &mut Session, p: &Value) -> CmdResult {
    let id = feature_id(p)?;
    let kind: FeatureKind = parse(p, "kind")?;
    let equations = equations_param(p)?;
    s.edit(|d| {
        let before = feature_values(d, id);
        let f = d.feature_mut(id).ok_or_else(|| CmdError(format!("{id} does not exist")))?;
        if std::mem::discriminant(&f.kind) != std::mem::discriminant(&kind) {
            return Err("a feature cannot change its type".into());
        }
        f.kind = kind;
        d.validate().map_err(CmdError)?;
        apply_feature_equations(d, id, &before, &equations)?;
        Ok(json!({}))
    })
}

fn param_value(p: &Value, key: &str) -> Result<Option<String>, CmdError> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(e)) if e.trim().is_empty() => Ok(None),
        Some(Value::String(e)) => Ok(Some(e.clone())),
        Some(Value::Number(n)) => Ok(Some(n.to_string())),
        Some(_) => Err(format!("`{key}` must be an equation (text) or a number").into()),
    }
}

fn param_list(s: &mut Session, _p: &Value) -> CmdResult {
    let d = s.document();
    let model: Vec<Value> = d
        .parameters()
        .model
        .iter()
        .map(|m| {
            let (value, unit) = d.value_at(&m.target).map_or((Value::Null, Value::Null), |(v, u)| (json!(v), json!(u.label())));
            json!({ "name": m.name, "of": d.describe_path(&m.target), "target": m.target, "unit": unit, "equation": m.equation, "value": value, "comment": m.comment })
        })
        .collect();
    let env = d.parameter_values();
    let user: Vec<Value> = d
        .parameters()
        .user
        .iter()
        .map(|u| json!({ "name": u.name, "unit": u.unit.label(), "equation": u.equation, "value": env.get(&u.name), "comment": u.comment }))
        .collect();
    Ok(json!({ "model": model, "user": user }))
}

fn param_unit(p: &Value) -> Result<ParamUnit, CmdError> {
    match p.get("unit").and_then(Value::as_str).unwrap_or("mm") {
        "mm" => Ok(ParamUnit::Mm),
        "deg" => Ok(ParamUnit::Deg),
        "ul" => Ok(ParamUnit::Ul),
        other => Err(format!("unknown unit `{other}` (mm, deg or ul)").into()),
    }
}

fn param_add(s: &mut Session, p: &Value) -> CmdResult {
    let name = field(p, "name")?.as_str().ok_or("`name` must be text")?.trim().to_owned();
    let equation = param_value(p, "equation")?.ok_or("a user parameter needs an equation")?;
    let comment = p.get("comment").and_then(Value::as_str).unwrap_or("").to_owned();
    let param = UserParam { name: name.clone(), equation, unit: param_unit(p)?, comment };
    s.edit(|d| d.add_user_parameter(param).map_err(CmdError))?;
    Ok(json!({ "name": name, "value": s.document().parameter_values().get(&name) }))
}

fn param_set(s: &mut Session, p: &Value) -> CmdResult {
    let name = field(p, "name")?.as_str().ok_or("`name` must be text")?.to_owned();
    let has_equation = p.get("equation").is_some();
    let equation = param_value(p, "equation")?;
    let comment = match p.get("comment") {
        None => None,
        Some(Value::String(c)) => Some(c.clone()),
        Some(_) => return Err("`comment` must be text".into()),
    };
    if !has_equation && comment.is_none() {
        return Err("give an equation or a comment".into());
    }
    s.edit(|d| {
        if let Some(c) = &comment {
            d.set_comment(&name, c).map_err(CmdError)?;
        }
        if !has_equation {
            return Ok(());
        }
        let target = d.parameters().model.iter().find(|m| m.name == name).map(|m| m.target.clone());
        match (target, equation.as_deref().map(|e| (e, e.trim().parse::<f64>()))) {
            // A plain number on a model value: the value itself, no equation.
            (Some(path), Some((_, Ok(v)))) => {
                d.set_equation(&name, None).map_err(CmdError)?;
                d.set_value_at(&path, v).map_err(CmdError)
            }
            (_, e) => d.set_equation(&name, e.map(|(e, _)| e)).map_err(CmdError),
        }
    })?;
    Ok(json!({ "name": name, "value": s.document().parameter_values().get(&name) }))
}

fn param_rename(s: &mut Session, p: &Value) -> CmdResult {
    let from = field(p, "name")?.as_str().ok_or("`name` must be text")?.to_owned();
    let to = field(p, "to")?.as_str().ok_or("`to` must be text")?.trim().to_owned();
    s.edit(|d| d.rename_parameter(&from, &to).map_err(CmdError))?;
    Ok(json!({ "name": to }))
}

fn param_delete(s: &mut Session, p: &Value) -> CmdResult {
    let name = field(p, "name")?.as_str().ok_or("`name` must be text")?.to_owned();
    s.edit(|d| d.delete_user_parameter(&name).map_err(CmdError))?;
    Ok(json!({}))
}

fn feature_rename(s: &mut Session, p: &Value) -> CmdResult {
    let id = feature_id(p)?;
    let name = field(p, "name")?.as_str().ok_or("`name` must be text")?.trim().to_owned();
    if name.is_empty() || name.len() > 200 {
        return Err("a name needs 1 to 200 characters".into());
    }
    s.edit(|d| {
        d.feature_mut(id).ok_or_else(|| CmdError(format!("{id} does not exist")))?.name = name;
        Ok(json!({}))
    })
}

fn feature_suppress(s: &mut Session, p: &Value) -> CmdResult {
    let id = feature_id(p)?;
    let on = p.get("suppressed").and_then(Value::as_bool).unwrap_or(true);
    s.edit(|d| {
        d.feature_mut(id).ok_or_else(|| CmdError(format!("{id} does not exist")))?.suppressed = on;
        Ok(json!({}))
    })
}

fn opt_feature(p: &Value, key: &str) -> Result<Option<FeatureId>, CmdError> {
    Ok(opt_id(p, key)?.map(FeatureId))
}

fn feature_end_of_part(s: &mut Session, p: &Value) -> CmdResult {
    let before = opt_feature(p, "before")?;
    s.edit(|d| d.set_end_of_part(before).map_err(CmdError))?;
    Ok(json!({ "computed": s.document().end_of_part() }))
}

fn feature_move(s: &mut Session, p: &Value) -> CmdResult {
    let (id, before) = (feature_id(p)?, opt_feature(p, "before")?);
    s.edit(|d| d.move_feature(id, before).map_err(CmdError))?;
    let order: Vec<u32> = s.document().features().iter().map(|f| f.id.0).collect();
    Ok(json!({ "order": order }))
}

fn feature_delete(s: &mut Session, p: &Value) -> CmdResult {
    let id = feature_id(p)?;
    s.edit(|d| d.remove(id).map(|_| json!({})).map_err(CmdError))
}

fn edit_undo(s: &mut Session, _: &Value) -> CmdResult {
    if s.undo() { Ok(json!({})) } else { Err("nothing to undo".into()) }
}

fn edit_redo(s: &mut Session, _: &Value) -> CmdResult {
    if s.redo() { Ok(json!({})) } else { Err("nothing to redo".into()) }
}

fn tree_value(s: &Session, regen: Option<&Regen>) -> Value {
    let features: Vec<Value> = s
        .document()
        .features()
        .iter()
        .map(|f| {
            json!({
                "id": f.id.0,
                "name": f.name,
                "type": f.kind.type_name(),
                "suppressed": f.suppressed,
                "depends_on": f.kind.depends_on().iter().map(|d| d.0).collect::<Vec<_>>(),
                "status": regen.and_then(|r| r.status_of(f.id)).map(|st| serde_json::to_value(st).unwrap_or(Value::Null)),
            })
        })
        .collect();
    json!({ "name": s.document().name, "features": features, "revision": s.revision() })
}

fn model_tree(s: &mut Session, _: &Value) -> CmdResult {
    Ok(tree_value(s, None))
}

// ---- geometry commands ------------------------------------------------------------------------

fn body_shapes(r: &Regen) -> Vec<ShapeHandle> {
    r.bodies.iter().map(|b| b.shape).collect()
}

fn model_regenerate(s: &mut Session, k: &mut dyn Kernel, _: &Value) -> CmdResult {
    let r = s.regen(k);
    let (bodies, millis, err) = (r.bodies.len(), r.millis, r.first_error().map(|(f, m)| (f.0, m.to_owned())));
    let snapshot = r.clone();
    let mut v = tree_value(s, Some(&snapshot));
    if let Some(o) = v.as_object_mut() {
        o.insert("bodies".into(), json!(bodies));
        o.insert("millis".into(), json!(millis));
        o.insert("error".into(), err.map(|(f, m)| json!({ "feature": f, "message": m })).unwrap_or(Value::Null));
    }
    Ok(v)
}

fn model_mass(s: &mut Session, k: &mut dyn Kernel, p: &Value) -> CmdResult {
    let density = opt_num(p, "density")?.unwrap_or(1.0);
    let shapes = body_shapes(s.regen(k));
    let mut out = Vec::new();
    for (i, b) in shapes.iter().enumerate() {
        let m = k.mass_properties(*b, density).map_err(|e| CmdError(e.to_string()))?;
        let c = m.center_of_mass;
        out.push(json!({ "body": i, "volume": m.volume, "area": m.area, "mass": m.mass, "center_of_mass": [c.x, c.y, c.z], "inertia": m.inertia }));
    }
    Ok(json!({ "bodies": out }))
}

fn model_topology(s: &mut Session, k: &mut dyn Kernel, _: &Value) -> CmdResult {
    let shapes = body_shapes(s.regen(k));
    let mut out = Vec::new();
    for (i, b) in shapes.iter().enumerate() {
        let t = k.topology(*b).map_err(|e| CmdError(e.to_string()))?;
        let bb = k.bounding_box(*b).map_err(|e| CmdError(e.to_string()))?;
        out.push(json!({
            "body": i, "kind": format!("{:?}", t.kind), "solids": t.solids, "faces": t.faces, "edges": t.edges, "vertices": t.vertices,
            "valid": k.is_valid(*b).map_err(|e| CmdError(e.to_string()))?,
            "bbox": bb.map(|b| json!({ "min": [b.min.x, b.min.y, b.min.z], "max": [b.max.x, b.max.y, b.max.z] })),
        }));
    }
    Ok(json!({ "bodies": out }))
}

fn model_faces(s: &mut Session, k: &mut dyn Kernel, p: &Value) -> CmdResult {
    let body = opt_id(p, "body")?.unwrap_or(0) as usize;
    let r = s.regen(k);
    let b = r.bodies.get(body).ok_or("no such body")?.clone();
    let mut out = Vec::new();
    for (i, name) in b.names.iter().enumerate() {
        let i = u32::try_from(i).map_err(|_| "too many faces")?;
        let info = k.face_info(b.shape.face(i)).map_err(|e| CmdError(e.to_string()))?;
        out.push(json!({ "face": i, "name": name, "surface": info.surface, "area": info.area, "centroid": [info.centroid.x, info.centroid.y, info.centroid.z] }));
    }
    Ok(json!({ "body": body, "faces": out }))
}

/// A face or edge to measure: `{"face": <face reference>}`, `{"edge": <edge reference>}`, or
/// `{"body": n, "face": index}` / `{"body": n, "edge": index}`.
fn entity_param(r: &Regen, k: &dyn Kernel, v: &Value, key: &str) -> Result<crate::measure::Entity, CmdError> {
    use crate::measure::Entity;
    let body = v.get("body").and_then(Value::as_u64).map_or(0, |b| b as usize);
    let index = |x: &Value| x.as_u64().and_then(|i| u32::try_from(i).ok());
    match (v.get("face"), v.get("edge")) {
        (Some(f), _) if f.is_object() => {
            let fr: FaceRef = serde_json::from_value(f.clone()).map_err(|e| CmdError(format!("`{key}.face`: {e}")))?;
            let (body, face) = r.resolve(&fr, k).map_err(CmdError)?;
            Ok(Entity::Face { body, face })
        }
        (Some(f), _) => {
            Ok(Entity::Face { body, face: index(f).ok_or_else(|| CmdError(format!("`{key}.face` must be an index or a face reference")))? })
        }
        (_, Some(e)) if e.is_object() => {
            let er: EdgeRef = serde_json::from_value(e.clone()).map_err(|x| CmdError(format!("`{key}.edge`: {x}")))?;
            let (body, edge) = r.resolve_edge(&er, k).map_err(CmdError)?;
            Ok(Entity::Edge { body, edge })
        }
        (_, Some(e)) => {
            Ok(Entity::Edge { body, edge: index(e).ok_or_else(|| CmdError(format!("`{key}.edge` must be an index or an edge reference")))? })
        }
        _ => Err(format!("`{key}` needs a face or an edge").into()),
    }
}

fn model_measure(s: &mut Session, k: &mut dyn Kernel, p: &Value) -> CmdResult {
    let r = s.regen(k).clone();
    let a = entity_param(&r, k, field(p, "a")?, "a")?;
    let b = match p.get("b") {
        None | Some(Value::Null) => None,
        Some(v) => Some(entity_param(&r, k, v, "b")?),
    };
    let m = crate::measure::measure(k, &r, a, b).map_err(CmdError)?;
    let values: Vec<Value> = m.values.iter().map(|(l, v, u)| json!({ "label": l, "value": v, "unit": u })).collect();
    let nearest = m.nearest.map(|(a, b)| json!([[a.x, a.y, a.z], [b.x, b.y, b.z]]));
    Ok(json!({ "values": values, "nearest": nearest }))
}

fn model_work(s: &mut Session, k: &mut dyn Kernel, _p: &Value) -> CmdResult {
    let r = s.regen(k);
    let v3 = |v: tenon_geom::Vec3| json!([v.x, v.y, v.z]);
    let out: Vec<Value> = r
        .work
        .iter()
        .map(|(id, g)| match g {
            crate::regen::WorkGeom::Plane(f) => {
                json!({ "feature": id.0, "type": "plane", "origin": v3(f.origin()), "normal": v3(f.z()), "x": v3(f.x()) })
            }
            crate::regen::WorkGeom::Axis(a) => json!({ "feature": id.0, "type": "axis", "origin": v3(a.origin()), "direction": v3(a.dir()) }),
            crate::regen::WorkGeom::Point(p) => json!({ "feature": id.0, "type": "point", "point": v3(*p) }),
        })
        .collect();
    Ok(json!({ "work": out }))
}

fn model_edges(s: &mut Session, k: &mut dyn Kernel, p: &Value) -> CmdResult {
    let body = opt_id(p, "body")?.unwrap_or(0) as usize;
    let r = s.regen(k).clone();
    let b = r.bodies.get(body).ok_or("no such body")?;
    let topo = k.topology(b.shape).map_err(|e| CmdError(e.to_string()))?;
    let mut out = Vec::new();
    for (i, adj) in topo.edge_faces.iter().enumerate() {
        let i = u32::try_from(i).map_err(|_| "too many edges")?;
        let info = k.edge_info(b.shape.edge(i)).map_err(|e| CmdError(e.to_string()))?;
        out.push(json!({
            "edge": i,
            "faces": naming::edge_names(&b.names, adj),
            "curve": info.curve,
            "length": info.length,
            "start": [info.start.x, info.start.y, info.start.z],
            "end": [info.end.x, info.end.y, info.end.z],
        }));
    }
    Ok(json!({ "body": body, "edges": out }))
}

/// The broken references of the failing feature, with the nearest replacements (DEC-034).
fn model_broken(s: &mut Session, k: &mut dyn Kernel, _p: &Value) -> CmdResult {
    let r = s.regen(k).clone();
    let Some((feature, message)) = r.first_error().map(|(f, m)| (f, m.to_owned())) else {
        return Ok(json!({ "feature": null, "broken": [] }));
    };
    let scene = crate::regen::scene(&r, k, &tenon_kernel::MeshTol::default()).map_err(CmdError)?;
    let broken: Vec<Value> = crate::repair::broken(s.document(), &scene, feature)
        .into_iter()
        .map(|b| {
            let candidates: Vec<Value> = b.candidates.into_iter().map(|c| json!({ "reference": c.reference, "distance": c.distance })).collect();
            json!({ "path": b.path, "kind": if b.kind == crate::repair::RefKind::Edge { "edge" } else { "face" }, "candidates": candidates })
        })
        .collect();
    let name = s.document().feature(feature).map(|f| f.name.clone()).unwrap_or_default();
    Ok(json!({ "feature": feature.0, "name": name, "message": message, "broken": broken }))
}

/// Puts a reference in place of a broken one, as one undoable edit.
fn model_repair(s: &mut Session, p: &Value) -> CmdResult {
    let id = feature_id(p)?;
    let path = p.get("path").and_then(Value::as_str).ok_or("missing parameter `path`")?.to_owned();
    let with = p.get("with").cloned().ok_or("missing parameter `with`: an edge or face reference")?;
    s.edit(|d| crate::repair::replace(d, id, &path, with).map(|()| json!({})).map_err(CmdError))
}

fn model_edge_ref(s: &mut Session, k: &mut dyn Kernel, p: &Value) -> CmdResult {
    let r = s.regen(k).clone();
    let er = if let Some(faces) = p.get("faces") {
        // By the names of its two faces; there must be exactly one such edge.
        let [a, b]: [FaceOrigin; 2] = serde_json::from_value(faces.clone()).map_err(|e| CmdError(format!("`faces`: {e}")))?;
        let want = naming::EdgeRef::new(a, b, naming::EdgeFingerprint { mid: tenon_geom::Vec3::ZERO, length: 0.0 }).faces;
        let mut found = Vec::new();
        for (bi, body) in r.bodies.iter().enumerate() {
            let topo = k.topology(body.shape).map_err(|e| CmdError(e.to_string()))?;
            for (ei, adj) in topo.edge_faces.iter().enumerate() {
                if naming::edge_names(&body.names, adj) == Some(want) {
                    found.push((bi, u32::try_from(ei).map_err(|_| "too many edges")?));
                }
            }
        }
        match found.as_slice() {
            [(b, e)] => r.edge_ref(*b, *e, k),
            [] => Err("no edge joins those two faces".into()),
            _ => Err("several edges join those two faces; choose one with `body` and `edge`".into()),
        }
    } else {
        r.edge_ref(opt_id(p, "body")?.unwrap_or(0) as usize, id_u32(p, "edge")?, k)
    }
    .map_err(CmdError)?;
    Ok(serde_json::to_value(er).unwrap_or(Value::Null))
}

fn model_face_ref(s: &mut Session, k: &mut dyn Kernel, p: &Value) -> CmdResult {
    let r = s.regen(k).clone();
    let fr = if p.get("origin").is_some() {
        r.face_ref_by_origin(parse(p, "origin")?, k)
    } else {
        r.face_ref(opt_id(p, "body")?.unwrap_or(0) as usize, id_u32(p, "face")?, k)
    }
    .map_err(CmdError)?;
    Ok(serde_json::to_value(fr).unwrap_or(Value::Null))
}

macro_rules! doc_cmd {
    ($id:literal, $label:literal, $help:literal, $mutates:expr, $f:expr) => {
        CommandSpec { id: $id, label: $label, help: $help, mutates: $mutates, run: Run::Doc($f) }
    };
}
macro_rules! geo_cmd {
    ($id:literal, $label:literal, $help:literal, $f:expr) => {
        CommandSpec { id: $id, label: $label, help: $help, mutates: false, run: Run::Geo($f) }
    };
}

static COMMANDS: &[CommandSpec] = &[
    doc_cmd!("document.rename", "Rename Part", "name: text", true, document_rename),
    doc_cmd!(
        "sketch.create",
        "New Sketch",
        "plane: \"xy\" | \"yz\" | \"xz\" (default xy), or face: a face reference from model.face_ref; project_origin (default true: a fixed point at the part origin, returned as `origin`)",
        true,
        sketch_create
    ),
    doc_cmd!("sketch.point", "Point", "sketch, x, y", true, sketch_point),
    doc_cmd!("sketch.line", "Line", "sketch; start (point id) or x1, y1; end (point id) or x2, y2", true, sketch_line),
    doc_cmd!("sketch.circle", "Circle", "sketch; center (point id) or cx, cy; r", true, sketch_circle),
    doc_cmd!("sketch.arc", "Arc", "sketch; center or cx, cy; start or x1, y1; end or x2, y2 (counter-clockwise)", true, sketch_arc),
    doc_cmd!("sketch.arc3", "Three-Point Arc", "sketch, x1, y1, x2, y2 (a point on the arc), x3, y3", true, sketch_arc3),
    doc_cmd!(
        "sketch.rectangle",
        "Rectangle",
        "sketch, x1, y1, x2, y2 (opposite corners); returns lines and corners (the first at x1, y1)",
        true,
        sketch_rectangle
    ),
    doc_cmd!("sketch.polygon", "Polygon", "sketch, cx, cy, x, y (a corner), sides", true, sketch_polygon),
    doc_cmd!("sketch.spline", "Spline", "sketch, points: [[x, y], ...] (control points), degree (default 3)", true, sketch_spline),
    doc_cmd!(
        "sketch.constrain",
        "Constrain",
        "sketch, constraint: {\"type\": \"horizontal\", \"line\": 3} etc. (see docs/commands.md); equation: drives a new dimension (e.g. \"width / 2\")",
        true,
        sketch_constrain
    ),
    doc_cmd!(
        "sketch.set_dimension",
        "Edit Dimension",
        "sketch, constraint (id); value (mm or rad), or equation (e.g. \"d0 / 2\", lengths in mm, angles in degrees)",
        true,
        sketch_set_dimension
    ),
    doc_cmd!("sketch.remove_constraint", "Delete Constraint", "sketch, constraint (id)", true, sketch_remove_constraint),
    doc_cmd!("sketch.drag", "Drag Point", "sketch, point (id), x, y", true, sketch_drag),
    doc_cmd!("sketch.delete", "Delete", "sketch, entities: [ids]", true, sketch_delete),
    doc_cmd!("sketch.construction", "Construction", "sketch, entity (id), on (default true)", true, sketch_construction),
    doc_cmd!("sketch.fillet", "Sketch Fillet", "sketch, point (corner id), radius", true, sketch_fillet),
    doc_cmd!("sketch.trim", "Trim", "sketch, curve (id), x, y (the piece to remove)", true, sketch_trim),
    doc_cmd!("sketch.offset", "Offset", "sketch, curves: [ids], distance (positive: outward / left)", true, sketch_offset),
    doc_cmd!("sketch.mirror", "Mirror", "sketch, entities: [ids], axis (line id)", true, sketch_mirror),
    doc_cmd!("sketch.info", "Sketch Info", "sketch", false, sketch_info),
    doc_cmd!(
        "model.extrude",
        "Extrude",
        "sketch; distance (plus backward: a second distance the other way), or symmetric: total, or through_all: true; reverse; operation: join | cut | new_body | intersect; regions: [[curve ids]]",
        true,
        model_extrude
    ),
    doc_cmd!(
        "model.revolve",
        "Revolve",
        "sketch; axis: line id or \"x\" | \"y\" | \"z\"; angle (rad, default full); symmetric; operation; regions",
        true,
        model_revolve
    ),
    doc_cmd!("model.fillet", "Fillet", "edges: [edge references from model.edge_ref], radius", true, model_fillet),
    doc_cmd!(
        "model.chamfer",
        "Chamfer",
        "edges: [edge references]; distance; and either distance2 or angle (rad) with reference: a face reference for the first distance",
        true,
        model_chamfer
    ),
    doc_cmd!(
        "model.shell",
        "Shell",
        "thickness; remove: [face references] (faces to open, default none); outside (default false: walls grow inwards)",
        true,
        model_shell
    ),
    doc_cmd!(
        "model.hole",
        "Hole",
        "sketch; points: [point ids] (default: the sketch's centre points, i.e. points on no curve); diameter; depth or through_all: true; type: simple | counterbore (counterbore_diameter, counterbore_depth) | countersink (countersink_diameter, countersink_angle: rad, default 90 deg); tip_angle (rad, default 118 deg) or flat_bottom: true; reverse (drill along the sketch normal)",
        true,
        model_hole
    ),
    doc_cmd!(
        "model.rib",
        "Rib",
        "sketch; lines: [line ids] (default: its open lines); thickness (half on each side of the sketch plane); distance (default: until it meets the part); flip",
        true,
        model_rib
    ),
    doc_cmd!(
        "model.sweep",
        "Sweep",
        "sketch (the profile); regions; path_sketch; path: [line and arc ids], joined end to end (default: every line and arc of path_sketch that is not construction); the sweep starts at the end nearer the profile; fixed (keep the profile's orientation; default: turn it with the path); operation",
        true,
        model_sweep
    ),
    doc_cmd!(
        "model.coil",
        "Coil",
        "sketch (the profile, off the axis); regions; axis: x | y | z, a line id of the sketch, or {\"work\": id}; pitch (mm per turn); turns; left (left-handed); operation",
        true,
        model_coil
    ),
    doc_cmd!(
        "model.loft",
        "Loft",
        "sections: [sketch ids], two or more in order, each with one closed profile; ruled (flat sides between sections); operation",
        true,
        model_loft
    ),
    doc_cmd!(
        "model.draft",
        "Draft",
        "faces: [face references] (the faces to tilt, on one body); the neutral plane, where the faces stay put: plane (\"xy\" | \"yz\" | \"xz\"), face (a planar face reference) or work_plane (id); angle (radians from the plane's normal, the pull direction); reverse (tilt the other way)",
        true,
        model_draft
    ),
    doc_cmd!(
        "model.split",
        "Split",
        "the cutting plane: plane (\"xy\" | \"yz\" | \"xz\"), face (a planar face reference) or work_plane (id); keep: \"both\" (two bodies, default) | \"front\" (the side the plane's normal points to) | \"back\"; body: a face reference on the one body to split (default: every body the plane passes through)",
        true,
        model_split
    ),
    doc_cmd!(
        "model.combine",
        "Combine",
        "base: a face reference on the body that stays; tools: [face references], one on each other body; operation: \"join\" (default) | \"cut\" | \"intersect\"; keep_tools (default false: the other bodies are used up)",
        true,
        model_combine
    ),
    doc_cmd!(
        "model.thread",
        "Thread",
        "face: a face reference on a round shaft or hole; pitch (mm; default: the ISO coarse pitch for the face's diameter, following it); designation (text for drawings; default \"M<diameter>x<pitch>\"); length (mm from the start end; default the whole face); reverse (start from the other end); left (left-handed); modelled (cut the groove into the part; default false: cosmetic)",
        true,
        model_thread
    ),
    doc_cmd!(
        "model.pattern.rect",
        "Rectangular Pattern",
        "features: [feature ids]; direction: \"x\" | \"y\" | \"z\" or an edge reference (straight edge); count; spacing; reverse; optional direction2, count2, spacing2, reverse2",
        true,
        model_pattern_rect
    ),
    doc_cmd!(
        "model.pattern.circular",
        "Circular Pattern",
        "features: [feature ids]; axis: \"x\" | \"y\" | \"z\", an edge reference (straight or circular edge) or a face reference (cylinder or cone); count; angle (rad, default a full turn: copies spread evenly); reverse",
        true,
        model_pattern_circular
    ),
    doc_cmd!(
        "model.mirror",
        "Mirror",
        "features: [feature ids]; plane: \"xy\" | \"yz\" | \"xz\", or face: a planar face reference",
        true,
        model_mirror
    ),
    doc_cmd!(
        "work.plane",
        "Work Plane",
        "by: offset (base: plane, distance) | angle (base: plane, axis: in the base plane, angle: rad) | midplane (a, b: parallel planes); a plane is \"xy\" | \"yz\" | \"xz\", a face reference or {\"work\": plane id}",
        true,
        work_plane
    ),
    doc_cmd!(
        "work.axis",
        "Work Axis",
        "axis: \"x\" | \"y\" | \"z\", an edge reference, a cylindrical face reference or {\"work\": id}; or a, b: two planes it lies on",
        true,
        work_axis
    ),
    doc_cmd!("work.point", "Work Point", "edge: a circular edge reference (its centre); or axis and plane (where they meet)", true, work_point),
    doc_cmd!(
        "feature.add",
        "Add Feature",
        "kind: a feature definition as in model.tree / the file format; equations: {value field: equation}",
        true,
        feature_add
    ),
    doc_cmd!("param.list", "Parameters", "every model parameter (named dimensions and feature values) and user parameter", false, param_list),
    doc_cmd!(
        "param.add",
        "Add Parameter",
        "name; equation (e.g. \"40 mm\", \"width / 2\"); unit: mm | deg | ul (default mm); comment",
        true,
        param_add
    ),
    doc_cmd!(
        "param.set",
        "Set Parameter",
        "name; equation: text, or a number (for a model parameter: its plain value, no equation), or null to drop a model parameter's equation; comment",
        true,
        param_set
    ),
    doc_cmd!("param.rename", "Rename Parameter", "name, to: the new name (every equation using it follows)", true, param_rename),
    doc_cmd!("param.delete", "Delete Parameter", "name: a user parameter no equation uses", true, param_delete),
    doc_cmd!(
        "feature.update",
        "Edit Feature",
        "feature (id), kind: the feature definition as in model.tree / the file format; equations: {value field: equation} (e.g. {\"/extent/distance\": \"d0 * 2\"})",
        true,
        feature_update
    ),
    doc_cmd!("feature.rename", "Rename Feature", "feature, name", true, feature_rename),
    doc_cmd!("feature.suppress", "Suppress", "feature, suppressed (default true)", true, feature_suppress),
    doc_cmd!("feature.delete", "Delete Feature", "feature", true, feature_delete),
    doc_cmd!(
        "feature.end_of_part",
        "Move End of Part",
        "before: the feature the marker goes just above (it and later ones are rolled back); omit or null for the end",
        true,
        feature_end_of_part
    ),
    doc_cmd!(
        "feature.move",
        "Reorder Feature",
        "feature; before: the feature it goes just above (omit or null: last, above the End of Part); sketches only it uses go along",
        true,
        feature_move
    ),
    doc_cmd!("edit.undo", "Undo", "", false, edit_undo),
    doc_cmd!("edit.redo", "Redo", "", false, edit_redo),
    doc_cmd!("model.tree", "Model Tree", "", false, model_tree),
    geo_cmd!("model.regenerate", "Regenerate", "", model_regenerate),
    geo_cmd!("model.mass", "Mass Properties", "density (mass per mm^3, default 1)", model_mass),
    geo_cmd!(
        "model.threads",
        "Threads",
        "every thread on the part: its feature, designation, pitch, major diameter, internal (in a hole), length, left, modelled, and where on its axis it starts and which way it runs",
        model_threads
    ),
    geo_cmd!("model.topology", "Topology", "", model_topology),
    geo_cmd!("model.faces", "Faces", "body (default 0)", model_faces),
    geo_cmd!("model.edges", "Edges", "body (default 0): every edge with the names of its two faces", model_edges),
    geo_cmd!("model.work", "Work Features", "where every work plane, axis and point is", model_work),
    geo_cmd!(
        "model.measure",
        "Measure",
        "a, and optionally b: {\"face\": face reference} | {\"edge\": edge reference} | {\"body\": n, \"face\" or \"edge\": index}; one gives its area, length or diameter, two give the distance (with the nearest points) and the angle",
        model_measure
    ),
    geo_cmd!(
        "model.broken",
        "Broken References",
        "the failing feature, its message, and each of its references that no longer finds its face or edge: path (in the feature definition), kind (edge or face) and up to three candidates (reference, distance in mm), nearest first",
        model_broken
    ),
    doc_cmd!(
        "model.repair",
        "Repair Reference",
        "feature; path (from model.broken); with: the edge or face reference to use instead (from model.broken, model.edge_ref or model.face_ref)",
        true,
        model_repair
    ),
    geo_cmd!(
        "model.edge_ref",
        "Edge Reference",
        "faces: [face origin, face origin] (the two faces the edge joins); or body (default 0) and edge (index from model.edges)",
        model_edge_ref
    ),
    geo_cmd!(
        "model.face_ref",
        "Face Reference",
        "origin: {\"type\": \"cap\", \"feature\": id, \"end\": \"start\" | \"end\"} or {\"type\": \"side\", \"feature\": id, \"curve\": id}; or body (default 0) and face (index from model.faces)",
        model_face_ref
    ),
];

/// This crate's commands.
pub fn commands() -> &'static [CommandSpec] {
    COMMANDS
}

pub fn find(id: &str) -> Option<&'static CommandSpec> {
    COMMANDS.iter().find(|c| c.id == id)
}
