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
    AxisRef, Chamfer, ChamferSize, Extrude, ExtrudeExtent, FeatureKind, Fillet, Operation, OriginAxis, OriginPlane, PlaneRef, RegionSel, Revolve,
    RevolveAngle, Shell,
};
use crate::naming::{self, EdgeRef, FaceOrigin, FaceRef};
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
    pub fn replace_document(&mut self, doc: Document, k: Option<&mut dyn Kernel>) {
        self.release_regen(k);
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
        match f(&mut self.doc) {
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

fn sketch_create(s: &mut Session, p: &Value) -> CmdResult {
    let plane = if let Some(face) = p.get("face") {
        PlaneRef::Face(serde_json::from_value::<FaceRef>(face.clone()).map_err(|e| CmdError(format!("`face`: {e}")))?)
    } else {
        let name = p.get("plane").and_then(Value::as_str).unwrap_or("xy");
        PlaneRef::Origin(match name.to_ascii_lowercase().as_str() {
            "xy" => OriginPlane::XY,
            "yz" => OriginPlane::YZ,
            "xz" => OriginPlane::XZ,
            _ => return Err(format!("unknown plane `{name}` (xy, yz or xz)").into()),
        })
    };
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
    Ok(json!({ "lines": lines.iter().map(|l| l.0).collect::<Vec<_>>() }))
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
    let id = on_sketch(s, p, |sk| Ok(sk.add_constraint(c)?))?;
    Ok(json!({ "constraint": id.0 }))
}

fn sketch_set_dimension(s: &mut Session, p: &Value) -> CmdResult {
    let (c, v) = (ConstraintId(id_u32(p, "constraint")?), num(p, "value")?);
    on_sketch(s, p, |sk| Ok(sk.set_dimension(c, v)?))?;
    Ok(json!({}))
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

fn add_feature(s: &mut Session, kind: FeatureKind) -> CmdResult {
    let id = s.edit(|d| d.add(kind).map_err(CmdError))?;
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
    add_feature(s, FeatureKind::Extrude(Extrude { sketch, regions: region_sel(p)?, extent, reverse, operation: operation(p)? }))
}

fn model_revolve(s: &mut Session, p: &Value) -> CmdResult {
    let sketch = sketch_id(p)?;
    let axis = match field(p, "axis")? {
        Value::String(a) => AxisRef::Origin(match a.to_ascii_lowercase().as_str() {
            "x" => OriginAxis::X,
            "y" => OriginAxis::Y,
            "z" => OriginAxis::Z,
            _ => return Err(format!("unknown axis `{a}` (x, y, z or a sketch line id)").into()),
        }),
        _ => AxisRef::SketchLine(EntityId(id_u32(p, "axis")?)),
    };
    let angle = match opt_num(p, "angle")? {
        None => RevolveAngle::Full,
        Some(a) if p.get("symmetric").and_then(Value::as_bool).unwrap_or(false) => RevolveAngle::Symmetric(a),
        Some(a) => RevolveAngle::Angle(a),
    };
    add_feature(s, FeatureKind::Revolve(Revolve { sketch, regions: region_sel(p)?, axis, angle, operation: operation(p)? }))
}

fn edge_refs(p: &Value) -> Result<Vec<EdgeRef>, CmdError> {
    let v: Vec<EdgeRef> = parse(p, "edges")?;
    if v.is_empty() || v.len() > 10_000 {
        return Err("`edges` needs 1 to 10000 edge references (from model.edge_ref)".into());
    }
    Ok(v)
}

fn model_fillet(s: &mut Session, p: &Value) -> CmdResult {
    add_feature(s, FeatureKind::Fillet(Fillet { edges: edge_refs(p)?, radius: num(p, "radius")? }))
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
    add_feature(s, FeatureKind::Chamfer(Chamfer { edges, size }))
}

fn model_shell(s: &mut Session, p: &Value) -> CmdResult {
    let remove: Vec<FaceRef> = if p.get("remove").is_some() { parse(p, "remove")? } else { Vec::new() };
    let outside = p.get("outside").and_then(Value::as_bool).unwrap_or(false);
    add_feature(s, FeatureKind::Shell(Shell { remove, thickness: num(p, "thickness")?, outside }))
}

fn feature_update(s: &mut Session, p: &Value) -> CmdResult {
    let id = feature_id(p)?;
    let kind: FeatureKind = parse(p, "kind")?;
    s.edit(|d| {
        let f = d.feature_mut(id).ok_or_else(|| CmdError(format!("{id} does not exist")))?;
        if std::mem::discriminant(&f.kind) != std::mem::discriminant(&kind) {
            return Err("a feature cannot change its type".into());
        }
        f.kind = kind;
        d.validate().map_err(CmdError)?;
        Ok(json!({}))
    })
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
    doc_cmd!("sketch.rectangle", "Rectangle", "sketch, x1, y1, x2, y2 (opposite corners)", true, sketch_rectangle),
    doc_cmd!("sketch.polygon", "Polygon", "sketch, cx, cy, x, y (a corner), sides", true, sketch_polygon),
    doc_cmd!("sketch.spline", "Spline", "sketch, points: [[x, y], ...] (control points), degree (default 3)", true, sketch_spline),
    doc_cmd!(
        "sketch.constrain",
        "Constrain",
        "sketch, constraint: {\"type\": \"horizontal\", \"line\": 3} etc. (see docs/commands.md)",
        true,
        sketch_constrain
    ),
    doc_cmd!("sketch.set_dimension", "Edit Dimension", "sketch, constraint (id), value (mm or rad)", true, sketch_set_dimension),
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
    doc_cmd!("feature.update", "Edit Feature", "feature (id), kind: the feature definition as in model.tree / the file format", true, feature_update),
    doc_cmd!("feature.rename", "Rename Feature", "feature, name", true, feature_rename),
    doc_cmd!("feature.suppress", "Suppress", "feature, suppressed (default true)", true, feature_suppress),
    doc_cmd!("feature.delete", "Delete Feature", "feature", true, feature_delete),
    doc_cmd!("edit.undo", "Undo", "", false, edit_undo),
    doc_cmd!("edit.redo", "Redo", "", false, edit_redo),
    doc_cmd!("model.tree", "Model Tree", "", false, model_tree),
    geo_cmd!("model.regenerate", "Regenerate", "", model_regenerate),
    geo_cmd!("model.mass", "Mass Properties", "density (mass per mm^3, default 1)", model_mass),
    geo_cmd!("model.topology", "Topology", "", model_topology),
    geo_cmd!("model.faces", "Faces", "body (default 0)", model_faces),
    geo_cmd!("model.edges", "Edges", "body (default 0): every edge with the names of its two faces", model_edges),
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
