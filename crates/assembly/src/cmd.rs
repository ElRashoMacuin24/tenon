//! Assembly commands (`asm.*`): placing, grounding, constraints and joints, moving, degrees of
//! freedom, the parts list, interference, the exploded view and editing parts in place. File
//! commands (open, save, insert a part file, export) are in `tenon_io::asm`.
//!
//! Units: millimetres and radians, as everywhere. Geometry is given as targets from `asm.geom`.

use serde_json::{Value, json};
use tenon_geom::{Frame, Vec3, tol};
use tenon_kernel::Kernel;
use tenon_model::schema::arg;
use tenon_model::{CmdError, CmdResult, FaceOrigin, FeatureId, OriginAxis, OriginPlane};

use crate::model::{Assembly, ComponentId, Geom, JointKind, MAX_COUNT, PatternId, PatternKind, RelKind, RelationshipId, Run, Target, Tweak};
use crate::session::{self, AsmSession, auto_explode, bom, dof, exploded, relate, scene_of, solve_assembly};

pub type AsmFn = fn(&mut AsmSession, Option<&mut dyn Kernel>, &Value) -> CmdResult;

/// An assembly command.
#[derive(Clone, Copy)]
pub struct AsmCommand {
    pub id: &'static str,
    pub label: &'static str,
    pub help: &'static str,
    /// Changes the assembly as one undo step.
    pub mutates: bool,
    /// Needs the geometry kernel.
    pub kernel: bool,
    pub run: AsmFn,
}

impl std::fmt::Debug for AsmCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AsmCommand").field("id", &self.id).finish()
    }
}

// ---- parameters -------------------------------------------------------------------------------

fn field<'v>(p: &'v Value, key: &str) -> Result<&'v Value, CmdError> {
    arg(p, key).filter(|v| !v.is_null()).ok_or_else(|| CmdError(format!("missing parameter `{key}`")))
}

fn num(p: &Value, key: &str) -> Result<f64, CmdError> {
    let v = field(p, key)?.as_f64().ok_or_else(|| CmdError(format!("`{key}` must be a number")))?;
    if tol::is_valid_coord(v) { Ok(v) } else { Err(CmdError(format!("`{key}` is out of range"))) }
}

fn opt_num(p: &Value, key: &str) -> Result<Option<f64>, CmdError> {
    match arg(p, key) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => num(p, key).map(Some),
    }
}

fn opt_bool(p: &Value, key: &str) -> Result<Option<bool>, CmdError> {
    match arg(p, key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_bool().map(Some).ok_or_else(|| CmdError(format!("`{key}` must be true or false"))),
    }
}

fn id(v: &Value, key: &str) -> Result<u32, CmdError> {
    v.as_u64().and_then(|n| u32::try_from(n).ok()).filter(|n| *n > 0).ok_or_else(|| CmdError(format!("`{key}` must be a positive whole number")))
}

fn component(p: &Value, key: &str) -> Result<ComponentId, CmdError> {
    id(field(p, key)?, key).map(ComponentId)
}

fn relationship(p: &Value, key: &str) -> Result<RelationshipId, CmdError> {
    id(field(p, key)?, key).map(RelationshipId)
}

/// A vector: `[x, y, z]`, `{"x":..,"y":..,"z":..}` or an axis name (`"x"`, `"-z"`, ...).
fn vec3(v: &Value, key: &str) -> Result<Vec3, CmdError> {
    let bad = || CmdError(format!("`{key}` must be [x, y, z] or an axis name like \"z\" or \"-x\""));
    let out = match v {
        Value::String(s) => match s.to_ascii_lowercase().as_str() {
            "x" | "+x" => Vec3::X,
            "y" | "+y" => Vec3::Y,
            "z" | "+z" => Vec3::Z,
            "-x" => -Vec3::X,
            "-y" => -Vec3::Y,
            "-z" => -Vec3::Z,
            _ => return Err(bad()),
        },
        Value::Array(a) if a.len() == 3 => {
            let c = |i: usize| a.get(i).and_then(Value::as_f64).ok_or_else(bad);
            Vec3::new(c(0)?, c(1)?, c(2)?)
        }
        Value::Object(_) => {
            let c = |k: &str| v.get(k).and_then(Value::as_f64).ok_or_else(bad);
            Vec3::new(c("x")?, c("y")?, c("z")?)
        }
        _ => return Err(bad()),
    };
    if [out.x, out.y, out.z].iter().all(|c| tol::is_valid_coord(*c)) { Ok(out) } else { Err(CmdError(format!("`{key}` is out of range"))) }
}

pub fn opt_vec3(p: &Value, key: &str) -> Result<Option<Vec3>, CmdError> {
    match arg(p, key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => vec3(v, key).map(Some),
    }
}

fn direction(p: &Value, key: &str) -> Result<Vec3, CmdError> {
    let v = vec3(field(p, key)?, key)?;
    if v.len() < tol::LINEAR {
        return Err(CmdError(format!("`{key}` has no direction")));
    }
    Ok(v.normalized())
}

fn target(p: &Value, key: &str) -> Result<Target, CmdError> {
    serde_json::from_value(field(p, key)?.clone()).map_err(|e| CmdError(format!("`{key}` is not a target (make one with asm.geom): {e}")))
}

fn v3(v: Vec3) -> Value {
    json!([v.x, v.y, v.z])
}

fn frame_json(f: &Frame) -> Value {
    json!({ "origin": v3(f.origin()), "x": v3(f.x()), "y": v3(f.y()), "z": v3(f.z()) })
}

// ---- reading ----------------------------------------------------------------------------------

fn asm_tree(s: &mut AsmSession, _k: Option<&mut dyn Kernel>, _p: &Value) -> CmdResult {
    let asm = s.assembly();
    let (per, total) = dof(asm, &s.parts);
    let failing = |id: RelationshipId| s.failing.iter().find(|(r, _)| *r == id).map(|(_, m)| m.clone());
    let element_of: std::collections::BTreeMap<ComponentId, PatternId> =
        asm.patterns.iter().flat_map(|p| p.elements.iter().flat_map(move |e| e.components.iter().map(move |c| (*c, p.id)))).collect();
    let components: Vec<Value> = asm
        .components
        .iter()
        .zip(&per)
        .map(|(c, d)| {
            json!({
                "id": c.id.0, "name": c.name, "part": c.part, "row": c.row, "grounded": c.grounded, "visible": c.visible,
                "mirrored": c.mirrored, "element_of": element_of.get(&c.id).map(|p| p.0),
                "placement": frame_json(&c.placement),
                "missing": s.parts.get(&c.key()).and_then(|p| p.missing.clone()),
                "dof": d.count(),
            })
        })
        .collect();
    let relationships: Vec<Value> = asm
        .relationships
        .iter()
        .map(|r| {
            let names: Vec<String> = r.kind.components().iter().filter_map(|c| asm.component(*c)).map(|c| c.name.clone()).collect();
            json!({ "id": r.id.0, "name": r.name, "type": r.kind.label(), "components": names, "suppressed": r.suppressed, "failing": failing(r.id) })
        })
        .collect();
    let patterns: Vec<Value> = asm
        .patterns
        .iter()
        .map(|p| {
            let ids = |c: &[ComponentId]| c.iter().map(|x| x.0).collect::<Vec<_>>();
            json!({
                "id": p.id.0, "name": p.name, "type": pattern_type(&p.kind), "suppressed": p.suppressed, "sources": ids(&p.sources),
                "elements": p.elements.iter().map(|e| json!({ "at": e.at, "components": ids(&e.components) })).collect::<Vec<_>>(),
                "failing": session::pattern_problem(asm, &s.parts, p),
            })
        })
        .collect();
    Ok(json!({ "name": asm.name, "components": components, "relationships": relationships, "patterns": patterns, "dof": total }))
}

fn pattern_type(kind: &PatternKind) -> &'static str {
    match kind {
        PatternKind::Rectangular { .. } => "rectangular",
        PatternKind::Circular { .. } => "circular",
        PatternKind::Mirror { .. } => "mirror",
    }
}

fn asm_dof(s: &mut AsmSession, _k: Option<&mut dyn Kernel>, _p: &Value) -> CmdResult {
    let asm = s.assembly();
    let (per, total) = dof(asm, &s.parts);
    let components: Vec<Value> = asm
        .components
        .iter()
        .zip(&per)
        .map(|(c, d)| {
            json!({
                "id": c.id.0, "name": c.name, "dof": d.count(),
                "translations": d.translations.iter().map(|v| v3(*v)).collect::<Vec<_>>(),
                "rotations": d.rotations.iter().map(|(a, p)| json!({ "axis": v3(*a), "point": v3(*p) })).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(json!({ "dof": total, "components": components }))
}

fn asm_bom(s: &mut AsmSession, _k: Option<&mut dyn Kernel>, _p: &Value) -> CmdResult {
    let rows: Vec<Value> = bom(s.assembly(), &s.parts)
        .iter()
        .map(|r| json!({ "item": r.item, "part": r.part, "name": r.name, "mirrored": r.mirrored, "quantity": r.quantity, "volume": r.volume, "material": r.material, "mass": r.mass, "components": r.components }))
        .collect();
    let total: usize = rows.iter().filter_map(|r| r["quantity"].as_u64()).map(|q| q as usize).sum();
    Ok(json!({ "rows": rows, "parts": rows.len(), "components": total }))
}

fn asm_mass(s: &mut AsmSession, _k: Option<&mut dyn Kernel>, _p: &Value) -> CmdResult {
    let asm = s.assembly();
    // Each component at its own part's density (g/cm^3; a part without a material is at 1).
    let (mut volume, mut mass, mut moment) = (0.0, 0.0, Vec3::ZERO);
    let mut rows = Vec::new();
    for c in asm.components.iter().filter(|c| c.visible) {
        let scene = scene_of(&s.parts, c).ok_or_else(|| CmdError(format!("{}: the part's geometry is not available", c.name)))?;
        let doc = s.parts.get(&c.key()).map(|p| p.session.document());
        let density = doc.map_or(tenon_model::materials::DEFAULT_DENSITY, tenon_model::Document::density);
        let v: f64 = scene.bodies.iter().map(|b| b.volume).sum();
        let grams = v * density / 1000.0;
        let m = scene.bodies.iter().fold(Vec3::ZERO, |a, b| a + c.placement.to_world(b.mass.center_of_mass) * (b.volume * density / 1000.0));
        volume += v;
        mass += grams;
        moment = moment + m;
        rows.push(json!({ "name": c.name, "volume": v, "mass": grams, "material": doc.and_then(|d| d.material()).map(|m| m.name.clone()) }));
    }
    let com = if mass > 0.0 { moment * (1.0 / mass) } else { Vec3::ZERO };
    Ok(json!({ "volume": volume, "mass": mass, "center_of_mass": v3(com), "components": rows }))
}

/// Builds a target from a component and a way to name its geometry.
fn asm_geom(s: &mut AsmSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let comp = match arg(p, "component") {
        None | Some(Value::Null) => None,
        Some(_) => Some(component(p, "component")?),
    };
    let plain = if let Some(v) = arg(p, "plane").filter(|v| !v.is_null()) {
        let plane = match v.as_str().map(str::to_ascii_lowercase).as_deref() {
            Some("xy") => OriginPlane::XY,
            Some("yz") => OriginPlane::YZ,
            Some("xz") => OriginPlane::XZ,
            _ => return Err("`plane` must be \"xy\", \"yz\" or \"xz\"".into()),
        };
        Some(Geom::Plane { plane })
    } else if let Some(v) = arg(p, "axis").filter(|v| !v.is_null()) {
        let axis = match v.as_str().map(str::to_ascii_lowercase).as_deref() {
            Some("x") => OriginAxis::X,
            Some("y") => OriginAxis::Y,
            Some("z") => OriginAxis::Z,
            _ => return Err("`axis` must be \"x\", \"y\" or \"z\"".into()),
        };
        Some(Geom::Axis { axis })
    } else if arg(p, "origin").is_some_and(|v| v.as_bool() == Some(true)) {
        Some(Geom::Origin)
    } else if let Some(v) = arg(p, "work").filter(|v| !v.is_null()) {
        Some(Geom::Work { feature: FeatureId(id(v, "work")?) })
    } else {
        None
    };
    let target =
        match (plain, comp) {
            (Some(geom), component) => Target { component, geom },
            (None, None) => return Err("give a component for a face or edge".into()),
            (None, Some(cid)) => {
                let c = s.component(cid)?;
                let scene = scene_of(&s.parts, c).ok_or("the part's geometry is not available")?;
                if let Some(f) = arg(p, "face").filter(|v| !v.is_null()) {
                    let (body, face) = if let Some(origin) = f.get("type").map(|_| f) {
                        let origin: FaceOrigin =
                            serde_json::from_value(origin.clone()).map_err(|e| CmdError(format!("`face` is not a face origin: {e}")))?;
                        let mut found = scene.bodies.iter().enumerate().flat_map(|(bi, b)| {
                            b.faces.iter().enumerate().filter(move |(_, (n, _))| *n == Some(origin)).map(move |(fi, _)| (bi, fi))
                        });
                        match (found.next(), found.next()) {
                            (Some(x), None) => x,
                            (None, _) => return Err("no face has that origin".into()),
                            (Some(_), Some(_)) => return Err("several faces have that origin; give `face` as {\"body\": b, \"index\": i}".into()),
                        }
                    } else {
                        let b = f.get("body").and_then(Value::as_u64).unwrap_or(0) as usize;
                        let i = f.get("index").and_then(Value::as_u64).ok_or("`face` needs an origin or an index")? as usize;
                        (b, i)
                    };
                    session::face_target(scene, cid, body, face).map_err(CmdError)?
                } else if let Some(e) = arg(p, "edge").filter(|v| !v.is_null()) {
                    let (body, edge) = if let Some(faces) = e.as_array() {
                        let names: Vec<FaceOrigin> = faces
                            .iter()
                            .map(|f| serde_json::from_value(f.clone()))
                            .collect::<Result<_, _>>()
                            .map_err(|e| CmdError(format!("`edge` must be two face origins: {e}")))?;
                        let [a, b] = names.as_slice() else { return Err("`edge` must be two face origins".into()) };
                        let want = tenon_model::EdgeRef::new(*a, *b, tenon_model::EdgeFingerprint { mid: Vec3::ZERO, length: 0.0 }).faces;
                        let mut found =
                            scene.bodies.iter().enumerate().flat_map(|(bi, b)| {
                                b.edges.iter().enumerate().filter(move |(_, (n, _))| *n == Some(want)).map(move |(ei, _)| (bi, ei))
                            });
                        match (found.next(), found.next()) {
                            (Some(x), None) => x,
                            (None, _) => return Err("no edge joins those faces".into()),
                            (Some(_), Some(_)) => return Err("several edges join those faces; give `edge` as {\"body\": b, \"index\": i}".into()),
                        }
                    } else {
                        let b = e.get("body").and_then(Value::as_u64).unwrap_or(0) as usize;
                        let i = e.get("index").and_then(Value::as_u64).ok_or("`edge` needs two face origins or an index")? as usize;
                        (b, i)
                    };
                    session::edge_target(scene, cid, body, edge).map_err(CmdError)?
                } else {
                    return Err("give face, edge, plane, axis, origin or work".into());
                }
            }
        };
    // Resolve it once, so a bad target fails here.
    let prim = session::resolve_target(s.assembly(), &s.parts, &target).map_err(CmdError)?.prim;
    let mut v = serde_json::to_value(&target).map_err(|e| CmdError(e.to_string()))?;
    if let Some(o) = v.as_object_mut() {
        o.insert("kind".into(), json!(prim.kind()));
    }
    Ok(v)
}

// ---- editing ----------------------------------------------------------------------------------

/// The result of an edit that may have moved components.
fn after(s: &AsmSession, extra: Value) -> CmdResult {
    let (_, total) = dof(s.assembly(), &s.parts);
    let mut v = extra;
    if let Some(o) = v.as_object_mut() {
        o.insert("dof".into(), json!(total));
    }
    Ok(v)
}

/// Strips the `kind` hint `asm.geom` adds, so its output can be passed straight back.
fn clean(mut p: Value) -> Value {
    for key in ["a", "b"] {
        if let Some(o) = p.get_mut(key).and_then(Value::as_object_mut) {
            o.remove("kind");
        }
    }
    p
}

fn asm_constrain(s: &mut AsmSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let p = clean(p.clone());
    let (a, b) = (target(&p, "a")?, target(&p, "b")?);
    let offset = opt_num(&p, "offset")?.unwrap_or(0.0);
    let kind = match field(&p, "type")?.as_str().unwrap_or("") {
        "mate" => RelKind::Mate { a, b, offset },
        "flush" => RelKind::Flush { a, b, offset },
        "angle" => RelKind::Angle { a, b, angle: num(&p, "angle")?, reference: Vec3::Z },
        "insert" => RelKind::Insert { a, b, offset, aligned: opt_bool(&p, "aligned")?.unwrap_or(false) },
        "tangent" => RelKind::Tangent { a, b, offset, inside: opt_bool(&p, "inside")?.unwrap_or(false) },
        "symmetry" => RelKind::Symmetry { a, b, plane: target(&p, "plane")? },
        other => return Err(CmdError(format!("unknown constraint type `{other}` (mate, flush, angle, insert, tangent, symmetry)"))),
    };
    let rid = s.edit(|asm, parts| relate(asm, parts, kind))?;
    s.failing.clear();
    let name = s.relationship(rid)?.name.clone();
    after(s, json!({ "relationship": rid.0, "name": name }))
}

fn asm_joint(s: &mut AsmSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let p = clean(p.clone());
    let (a, b) = (target(&p, "a")?, target(&p, "b")?);
    let t = field(&p, "type")?.as_str().unwrap_or("");
    let joint =
        JointKind::from_id(t).ok_or_else(|| CmdError(format!("unknown joint type `{t}` (rigid, revolute, slider, cylindrical, planar, ball)")))?;
    let kind = RelKind::Joint {
        joint,
        a,
        b,
        flip: opt_bool(&p, "flip")?.unwrap_or(false),
        offset: opt_num(&p, "offset")?.unwrap_or(0.0),
        angle: opt_num(&p, "angle")?.unwrap_or(0.0),
    };
    let rid = s.edit(|asm, parts| relate(asm, parts, kind))?;
    s.failing.clear();
    let name = s.relationship(rid)?.name.clone();
    after(s, json!({ "relationship": rid.0, "name": name }))
}

/// Refuses what cannot be done to an element of a pattern, which stands where the pattern says.
fn not_an_element(asm: &Assembly, c: ComponentId, what: &str) -> Result<(), CmdError> {
    match asm.pattern_of(c) {
        Some(p) => {
            let name = asm.component(c).map_or_else(|| c.to_string(), |x| x.name.clone());
            Err(CmdError(format!("{name} is an element of {}: {what}", p.name)))
        }
        None => Ok(()),
    }
}

/// Re-solves inside an edit; refuses the edit when it does not hold.
pub fn solve_or_refuse(asm: &mut crate::model::Assembly, parts: &session::Parts, drag: Option<ComponentId>) -> Result<(), CmdError> {
    let solved = solve_assembly(asm, parts, drag);
    if solved.converged {
        return Ok(());
    }
    let names: Vec<String> = solved.failing.iter().filter_map(|(r, _)| asm.relationship(*r)).map(|r| r.name.clone()).collect();
    Err(CmdError(format!("the relationships cannot all hold ({})", names.join(", "))))
}

fn asm_edit_relationship(s: &mut AsmSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let rid = relationship(p, "relationship")?;
    let (offset, angle, flip, aligned) = (opt_num(p, "offset")?, opt_num(p, "angle")?, opt_bool(p, "flip")?, opt_bool(p, "aligned")?);
    let inside = opt_bool(p, "inside")?;
    let joint = match arg(p, "type").and_then(Value::as_str) {
        Some(t) => Some(JointKind::from_id(t).ok_or_else(|| CmdError(format!("unknown joint type `{t}`")))?),
        None => None,
    };
    s.edit(|asm, parts| {
        let r = asm.relationship_mut(rid).ok_or_else(|| CmdError(format!("{rid} does not exist")))?;
        match &mut r.kind {
            RelKind::Mate { offset: o, .. } | RelKind::Flush { offset: o, .. } => *o = offset.unwrap_or(*o),
            RelKind::Angle { angle: a, .. } => *a = angle.unwrap_or(*a),
            RelKind::Insert { offset: o, aligned: al, .. } => {
                *o = offset.unwrap_or(*o);
                *al = aligned.unwrap_or(*al);
            }
            RelKind::Tangent { offset: o, inside: i, .. } => {
                *o = offset.unwrap_or(*o);
                *i = inside.unwrap_or(*i);
            }
            // (Nothing of a symmetry is a value: it is its three picks.)
            RelKind::Symmetry { .. } => {}
            RelKind::Joint { joint: j, flip: f, offset: o, angle: a, .. } => {
                *j = joint.unwrap_or(*j);
                *f = flip.unwrap_or(*f);
                *o = offset.unwrap_or(*o);
                *a = angle.unwrap_or(*a);
            }
        }
        solve_or_refuse(asm, parts, None)
    })?;
    s.failing.clear();
    after(s, json!({ "relationship": rid.0 }))
}

fn asm_suppress(s: &mut AsmSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let rid = relationship(p, "relationship")?;
    let on = opt_bool(p, "suppressed")?.unwrap_or(true);
    s.edit(|asm, parts| {
        asm.relationship_mut(rid).ok_or_else(|| CmdError(format!("{rid} does not exist")))?.suppressed = on;
        solve_or_refuse(asm, parts, None)
    })?;
    after(s, json!({ "relationship": rid.0, "suppressed": on }))
}

fn asm_delete(s: &mut AsmSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    if arg(p, "relationship").is_some_and(|v| !v.is_null()) {
        let rid = relationship(p, "relationship")?;
        s.edit(|asm, _| {
            let before = asm.relationships.len();
            asm.relationships.retain(|r| r.id != rid);
            if asm.relationships.len() == before { Err(CmdError(format!("{rid} does not exist"))) } else { Ok(()) }
        })?;
        s.failing.retain(|(r, _)| *r != rid);
        return after(s, json!({ "deleted": rid.0 }));
    }
    let cid = component(p, "component")?;
    not_an_element(s.assembly(), cid, "change the pattern's count, or delete the pattern")?;
    s.edit(|asm, _| if asm.remove_component(cid) { Ok(()) } else { Err(CmdError(format!("{cid} does not exist"))) })?;
    close_unused(s);
    after(s, json!({ "deleted": cid.0 }))
}

/// Parts no component uses any more are closed.
fn close_unused(s: &mut AsmSession) {
    let used: std::collections::BTreeSet<String> = s.assembly().components.iter().flat_map(|c| [c.part.clone(), c.key()]).collect();
    s.parts.retain(|k, p| used.contains(k) || p.session.is_dirty());
}

// ---- patterns and mirrors ---------------------------------------------------------------------

fn pattern_param(p: &Value, key: &str) -> Result<PatternId, CmdError> {
    id(field(p, key)?, key).map(PatternId)
}

fn opt_count(p: &Value, key: &str) -> Result<Option<u32>, CmdError> {
    match arg(p, key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| (1..=MAX_COUNT).contains(n))
            .map(Some)
            .ok_or_else(|| CmdError(format!("`{key}` must be a whole number from 1 to {MAX_COUNT}"))),
    }
}

/// One direction of a rectangular pattern from its parameters (those named with `suffix`), what
/// is not given as it `was`. A null direction takes the direction away.
fn run_param(p: &Value, suffix: &str, was: Option<&Run>) -> Result<Option<Run>, CmdError> {
    let (direction, count, spacing, reverse) =
        (format!("direction{suffix}"), format!("count{suffix}"), format!("spacing{suffix}"), format!("reverse{suffix}"));
    let along = match arg(p, &direction) {
        Some(Value::Null) => None,
        Some(_) => Some(target(p, &direction)?),
        None => was.map(|r| r.along.clone()),
    };
    let (count_given, spacing_given) = (opt_count(p, &count)?, opt_num(p, &spacing)?);
    let Some(along) = along else {
        if count_given.is_some() || spacing_given.is_some() {
            return Err(CmdError(format!("`{count}` and `{spacing}` need a `{direction}`")));
        }
        return Ok(None);
    };
    let count = count_given.or(was.map(|r| r.count)).ok_or_else(|| CmdError(format!("missing parameter `{count}`")))?;
    let spacing = spacing_given.or(was.map(|r| r.spacing)).ok_or_else(|| CmdError(format!("missing parameter `{spacing}`")))?;
    let reverse = opt_bool(p, &reverse)?.or(was.map(|r| r.reverse)).unwrap_or(false);
    Ok(Some(Run { along, reverse, count, spacing }))
}

/// Refuses parameters that mean nothing for `what`, rather than pass over them.
fn not_for(p: &Value, what: &str, keys: &[&str]) -> Result<(), CmdError> {
    match keys.iter().find(|k| arg(p, k).is_some_and(|v| !v.is_null())) {
        Some(k) => Err(CmdError(format!("`{k}` means nothing for {what}"))),
        None => Ok(()),
    }
}

/// A rectangular or circular rule from its parameters, what is not given as it `was`.
fn pattern_kind(p: &Value, was: Option<&PatternKind>) -> Result<PatternKind, CmdError> {
    let (first, second) = match was {
        Some(PatternKind::Rectangular { first, second }) => (Some(first), second.as_ref()),
        _ => (None, None),
    };
    let (axis, reverse, count, angle) = match was {
        Some(PatternKind::Circular { axis, reverse, count, angle }) => (Some(axis), Some(*reverse), Some(*count), Some(*angle)),
        _ => (None, None, None, None),
    };
    let circular = match arg(p, "type").and_then(Value::as_str) {
        Some("circular") => true,
        Some("rectangular") => false,
        Some(other) => return Err(CmdError(format!("unknown pattern type `{other}` (rectangular, circular)"))),
        None => axis.is_some(),
    };
    if circular {
        not_for(p, "a circular pattern", &["direction", "spacing", "direction2", "count2", "spacing2", "reverse2"])?;
        let axis = match arg(p, "axis").filter(|v| !v.is_null()) {
            Some(_) => target(p, "axis")?,
            None => axis.cloned().ok_or("missing parameter `axis`")?,
        };
        let count = opt_count(p, "count")?.or(count).ok_or("missing parameter `count`")?;
        let angle = opt_num(p, "angle")?.or(angle).unwrap_or(std::f64::consts::TAU);
        let reverse = opt_bool(p, "reverse")?.or(reverse).unwrap_or(false);
        return Ok(PatternKind::Circular { axis, reverse, count, angle });
    }
    not_for(p, "a rectangular pattern", &["axis", "angle"])?;
    let first = run_param(p, "", first)?.ok_or("missing parameter `direction`")?;
    Ok(PatternKind::Rectangular { first, second: run_param(p, "2", second)? })
}

/// The ids of a list of components, in order.
fn ids_param(s: &AsmSession, p: &Value, key: &str) -> Result<Vec<ComponentId>, CmdError> {
    let list = field(p, key)?.as_array().ok_or_else(|| CmdError(format!("`{key}` must be a list of component ids")))?;
    let ids: Vec<ComponentId> = list.iter().map(|v| id(v, key).map(ComponentId)).collect::<Result<_, _>>()?;
    for c in &ids {
        s.component(*c)?;
    }
    Ok(ids)
}

/// What a pattern made or changed comes to.
fn pattern_result(s: &AsmSession, id: PatternId) -> CmdResult {
    let p = s.assembly().pattern(id).ok_or_else(|| CmdError(format!("{id} does not exist")))?;
    let components: Vec<u32> = p.elements.iter().flat_map(|e| e.components.iter().map(|c| c.0)).collect();
    after(s, json!({ "pattern": id.0, "name": p.name, "elements": p.elements.len(), "components": components }))
}

fn asm_pattern(s: &mut AsmSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let sources = components_param(s, p)?;
    let kind = pattern_kind(p, None)?;
    let id = s.edit(|asm, parts| {
        let id = session::add_pattern(asm, parts, sources, kind)?;
        solve_or_refuse(asm, parts, None)?;
        Ok(id)
    })?;
    s.failing.clear();
    pattern_result(s, id)
}

fn asm_mirror(s: &mut AsmSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let sources = components_param(s, p)?;
    let plane = target(p, "plane")?;
    let reuse = if arg(p, "reuse").is_some_and(|v| !v.is_null()) { ids_param(s, p, "reuse")? } else { Vec::new() };
    if let Some(c) = reuse.iter().find(|c| !sources.contains(c)) {
        return Err(CmdError(format!("{c} is in `reuse` but not among `components`")));
    }
    let kind = session::mirror_kind(s.assembly(), &s.parts, plane, &reuse).map_err(CmdError)?;
    let id = s.edit(|asm, parts| {
        let id = session::add_pattern(asm, parts, sources, kind)?;
        solve_or_refuse(asm, parts, None)?;
        Ok(id)
    })?;
    s.failing.clear();
    pattern_result(s, id)
}

/// Changes a pattern or mirror: what is given changes, the rest stays.
fn asm_edit_pattern(s: &mut AsmSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let id = pattern_param(p, "pattern")?;
    let was = s.assembly().pattern(id).cloned().ok_or_else(|| CmdError(format!("{id} does not exist")))?;
    let sources = if arg(p, "components").is_some_and(|v| !v.is_null()) { ids_param(s, p, "components")? } else { was.sources.clone() };
    let kind = match &was.kind {
        PatternKind::Mirror { plane, reuse } => {
            not_for(
                p,
                "a mirror",
                &["type", "direction", "count", "spacing", "reverse", "direction2", "count2", "spacing2", "reverse2", "axis", "angle"],
            )?;
            let plane = if arg(p, "plane").is_some_and(|v| !v.is_null()) { target(p, "plane")? } else { plane.clone() };
            let again = if arg(p, "reuse").is_some_and(|v| !v.is_null()) {
                ids_param(s, p, "reuse")?
            } else {
                reuse.iter().map(|r| r.component).filter(|c| sources.contains(c)).collect()
            };
            if let Some(c) = again.iter().find(|c| !sources.contains(c)) {
                return Err(CmdError(format!("{c} is in `reuse` but not among what is mirrored")));
            }
            session::mirror_kind(s.assembly(), &s.parts, plane, &again).map_err(CmdError)?
        }
        kind => {
            not_for(p, "a pattern", &["plane", "reuse"])?;
            pattern_kind(p, Some(kind))?
        }
    };
    s.edit(|asm, parts| {
        session::change_pattern(asm, parts, id, sources, kind)?;
        solve_or_refuse(asm, parts, None)
    })?;
    s.failing.clear();
    close_unused(s);
    pattern_result(s, id)
}

fn asm_suppress_pattern(s: &mut AsmSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let id = pattern_param(p, "pattern")?;
    let on = opt_bool(p, "suppressed")?.unwrap_or(true);
    s.edit(|asm, _| session::suppress_pattern(asm, id, on))?;
    after(s, json!({ "pattern": id.0, "suppressed": on }))
}

fn asm_delete_pattern(s: &mut AsmSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let id = pattern_param(p, "pattern")?;
    let keep = opt_bool(p, "keep_elements")?.unwrap_or(false);
    s.edit(|asm, _| if asm.remove_pattern(id, keep) { Ok(()) } else { Err(CmdError(format!("{id} does not exist"))) })?;
    close_unused(s);
    after(s, json!({ "deleted": id.0, "kept_elements": keep }))
}

/// The rows of the design table of the part file at `part` (empty without a table).
pub fn rows_of(parts: &crate::Parts, part: &str) -> Vec<String> {
    parts.get(part).and_then(|p| p.session.document().table()).map(|t| t.rows.iter().map(|r| r.name.clone()).collect()).unwrap_or_default()
}

/// Checks that the part file at `part` can be had at `row`: says which rows there are when not.
pub fn check_row(parts: &crate::Parts, part: &str, row: &str) -> Result<(), CmdError> {
    let file = part.rsplit(['/', '\\']).next().unwrap_or(part);
    let rows = rows_of(parts, part);
    if rows.is_empty() {
        return Err(CmdError(format!("{file} has no design table, so it comes in one size only")));
    }
    if !rows.iter().any(|r| r == row) {
        return Err(CmdError(format!("{file} has no row `{row}` in its design table: it has {}", rows.join(", "))));
    }
    Ok(())
}

/// Which size of its part a component is: a row of the part's design table, or (null) the part
/// as its file has it.
fn asm_set_row(s: &mut AsmSession, k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let cid = component(p, "component")?;
    let row = match arg(p, "row") {
        None | Some(Value::Null) => None,
        Some(Value::String(r)) => Some(r.clone()),
        Some(_) => return Err("`row` must be the name of a row, or null".into()),
    };
    let part = s.component(cid)?.part.clone();
    not_an_element(s.assembly(), cid, "it is the size of what it repeats")?;
    if let Some(row) = &row {
        check_row(&s.parts, &part, row)?;
    }
    s.edit(|asm, parts| {
        asm.component_mut(cid).ok_or_else(|| CmdError(format!("{cid} does not exist")))?.row = row.clone();
        // (What repeats it is that size too.)
        session::place_elements(asm, parts);
        Ok(())
    })?;
    // The part in that size, then the relationships that hold the component.
    if let Some(k) = k {
        s.refresh(k).map_err(CmdError)?;
    }
    let solved = s.update()?;
    Ok(json!({ "component": cid.0, "row": row, "rows": rows_of(&s.parts, &part), "converged": solved.converged }))
}

fn asm_rename(s: &mut AsmSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let name =
        field(p, "name")?.as_str().map(str::trim).filter(|n| !n.is_empty() && n.len() <= 256).ok_or("`name` must be 1 to 256 characters")?.to_owned();
    if arg(p, "relationship").is_some_and(|v| !v.is_null()) {
        let rid = relationship(p, "relationship")?;
        s.edit(|asm, _| {
            asm.relationship_mut(rid).ok_or_else(|| CmdError(format!("{rid} does not exist")))?.name = name.clone();
            Ok(())
        })?;
    } else if arg(p, "component").is_some_and(|v| !v.is_null()) {
        let cid = component(p, "component")?;
        s.edit(|asm, _| {
            asm.component_mut(cid).ok_or_else(|| CmdError(format!("{cid} does not exist")))?.name = name.clone();
            Ok(())
        })?;
    } else if arg(p, "pattern").is_some_and(|v| !v.is_null()) {
        let pid = pattern_param(p, "pattern")?;
        s.edit(|asm, _| {
            asm.pattern_mut(pid).ok_or_else(|| CmdError(format!("{pid} does not exist")))?.name = name.clone();
            Ok(())
        })?;
    } else {
        s.edit(|asm, _| {
            asm.name = name.clone();
            Ok(())
        })?;
    }
    Ok(json!({ "name": name }))
}

fn asm_ground(s: &mut AsmSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let cid = component(p, "component")?;
    let on = opt_bool(p, "grounded")?.unwrap_or(true);
    not_an_element(s.assembly(), cid, "the pattern holds it")?;
    s.edit(|asm, parts| {
        asm.component_mut(cid).ok_or_else(|| CmdError(format!("{cid} does not exist")))?.grounded = on;
        solve_or_refuse(asm, parts, None)
    })?;
    after(s, json!({ "component": cid.0, "grounded": on }))
}

fn asm_visible(s: &mut AsmSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let cid = component(p, "component")?;
    let on = opt_bool(p, "visible")?.unwrap_or(true);
    if s.assembly().suppressed_elements().contains(&cid) {
        return Err(CmdError(format!("{cid} is an element of a suppressed pattern: bring the pattern back first")));
    }
    s.edit(|asm, _| {
        asm.component_mut(cid).ok_or_else(|| CmdError(format!("{cid} does not exist")))?.visible = on;
        Ok(())
    })?;
    Ok(json!({ "component": cid.0, "visible": on }))
}

/// Moves a component as a drag would: by `by`, turned by `turn`, then the relationships pull it
/// (and what it is attached to) back where they must.
fn asm_move(s: &mut AsmSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let cid = component(p, "component")?;
    let by = opt_vec3(p, "by")?.unwrap_or(Vec3::ZERO);
    let turn = match arg(p, "turn").filter(|v| !v.is_null()) {
        Some(t) => {
            let axis = direction(t, "axis")?;
            let angle = num(t, "angle")?;
            let through = opt_vec3(t, "through")?;
            Some((axis, angle, through))
        }
        None => None,
    };
    not_an_element(s.assembly(), cid, "it stands where the pattern puts it; move what it repeats")?;
    s.edit(|asm, parts| {
        let c = asm.component(cid).ok_or_else(|| CmdError(format!("{cid} does not exist")))?;
        if c.grounded {
            return Err(CmdError(format!("{} is grounded", c.name)));
        }
        let center = session::local_bbox(parts, c).map_or(c.placement.origin(), |b| c.placement.to_world(b.center()));
        let mut f = c.placement;
        if let Some((axis, angle, through)) = turn {
            let r = crate::math::M3::exp(axis * angle);
            let pivot = through.unwrap_or(center);
            let m = crate::solve::Motion { r, t: pivot - r.apply(pivot) };
            f = m.apply_frame(&f);
        }
        f = f.with_origin(f.origin() + by).ok_or("the move is out of range")?;
        if let Some(c) = asm.component_mut(cid) {
            c.placement = f;
        }
        solve_or_refuse(asm, parts, Some(cid))
    })?;
    let f = s.component(cid)?.placement;
    after(s, json!({ "component": cid.0, "placement": frame_json(&f) }))
}

/// Puts a component exactly at a placement (then the relationships are solved again).
fn asm_place(s: &mut AsmSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let cid = component(p, "component")?;
    let origin = vec3(field(p, "origin")?, "origin")?;
    let z = opt_vec3(p, "z")?.unwrap_or(Vec3::Z);
    let x = opt_vec3(p, "x")?.unwrap_or(if z.normalized().x.abs() > 0.9 { Vec3::Y } else { Vec3::X });
    let f = Frame::new(origin, z, x).ok_or("`x` and `z` must be non-zero and not parallel")?;
    not_an_element(s.assembly(), cid, "it stands where the pattern puts it; move what it repeats")?;
    s.edit(|asm, parts| {
        asm.component_mut(cid).ok_or_else(|| CmdError(format!("{cid} does not exist")))?.placement = f;
        solve_or_refuse(asm, parts, Some(cid))
    })?;
    let f = s.component(cid)?.placement;
    after(s, json!({ "component": cid.0, "placement": frame_json(&f) }))
}

fn asm_update(s: &mut AsmSession, _k: Option<&mut dyn Kernel>, _p: &Value) -> CmdResult {
    let solved = s.update()?;
    let failing: Vec<Value> = solved
        .failing
        .iter()
        .map(|(r, m)| json!({ "relationship": r.0, "name": s.assembly().relationship(*r).map(|x| x.name.clone()), "message": m }))
        .collect();
    after(s, json!({ "converged": solved.converged, "failing": failing }))
}

fn asm_undo(s: &mut AsmSession, _k: Option<&mut dyn Kernel>, _p: &Value) -> CmdResult {
    if !s.undo() {
        return Err("nothing to undo".into());
    }
    Ok(json!({ "revision": s.revision() }))
}

fn asm_redo(s: &mut AsmSession, _k: Option<&mut dyn Kernel>, _p: &Value) -> CmdResult {
    if !s.redo() {
        return Err("nothing to redo".into());
    }
    Ok(json!({ "revision": s.revision() }))
}

// ---- exploded view ----------------------------------------------------------------------------

pub fn components_param(s: &AsmSession, p: &Value) -> Result<Vec<ComponentId>, CmdError> {
    let list = field(p, "components")?.as_array().ok_or("`components` must be a list of component ids")?;
    let ids: Vec<ComponentId> = list.iter().map(|v| id(v, "components").map(ComponentId)).collect::<Result<_, _>>()?;
    if ids.is_empty() {
        return Err("`components` is empty".into());
    }
    for c in &ids {
        s.component(*c)?;
    }
    Ok(ids)
}

fn asm_explode_add(s: &mut AsmSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let components = components_param(s, p)?;
    let tweak = Tweak { components, direction: direction(p, "direction")?, distance: num(p, "distance")? };
    s.edit(|asm, _| {
        asm.explode.push(tweak);
        Ok(())
    })?;
    Ok(json!({ "step": s.assembly().explode.len() - 1, "steps": s.assembly().explode.len() }))
}

fn asm_explode_auto(s: &mut AsmSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let spacing = match opt_num(p, "spacing")? {
        Some(d) => d,
        None => session::assembly_bbox(s.assembly(), &s.parts).map_or(20.0, |b| (b.diagonal() * 0.25).max(5.0)),
    };
    let tweaks = auto_explode(s.assembly(), &s.parts, spacing);
    s.edit(|asm, _| {
        asm.explode = tweaks;
        Ok(())
    })?;
    Ok(json!({ "steps": s.assembly().explode.len(), "spacing": spacing }))
}

fn asm_explode_clear(s: &mut AsmSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let step = match arg(p, "step") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_u64().ok_or("`step` must be a whole number")? as usize),
    };
    s.edit(|asm, _| {
        match step {
            Some(i) if i < asm.explode.len() => {
                asm.explode.remove(i);
            }
            Some(i) => return Err(CmdError(format!("there is no step {i}"))),
            None => asm.explode.clear(),
        }
        Ok(())
    })?;
    Ok(json!({ "steps": s.assembly().explode.len() }))
}

fn asm_explode_positions(s: &mut AsmSession, _k: Option<&mut dyn Kernel>, _p: &Value) -> CmdResult {
    let frames = exploded(s.assembly());
    let rows: Vec<Value> = s
        .assembly()
        .components
        .iter()
        .filter_map(|c| frames.get(&c.id).map(|f| json!({ "id": c.id.0, "name": c.name, "origin": v3(f.origin()) })))
        .collect();
    Ok(json!({ "components": rows }))
}

// ---- kernel -----------------------------------------------------------------------------------

fn asm_interference(s: &mut AsmSession, k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let k = k.ok_or("asm.interference needs the geometry kernel")?;
    let only = match arg(p, "components") {
        None | Some(Value::Null) => None,
        Some(_) => Some(components_param(s, p)?),
    };
    let asm = s.assembly().clone();
    let clashes = crate::interfere::interference(k, &asm, &mut s.parts, only.as_deref()).map_err(CmdError)?;
    let name = |c: ComponentId| asm.component(c).map_or_else(|| c.to_string(), |c| c.name.clone());
    let rows: Vec<Value> = clashes.iter().map(|c| json!({ "a": name(c.a), "b": name(c.b), "volume": c.volume })).collect();
    let total = clashes.iter().fold(0.0, |a, c| a + c.volume);
    Ok(json!({ "count": rows.len(), "volume": total, "clashes": rows }))
}

/// Runs a part command on a component's part, in place: the part changes for every component
/// (and every assembly) that uses it, and the assembly is solved again.
fn asm_edit_part(s: &mut AsmSession, k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let cid = component(p, "component")?;
    let run = field(p, "run")?.as_str().ok_or("`run` must be a command id")?.to_owned();
    if run.starts_with("asm.") || run.starts_with("file.") {
        return Err("`run` must be a part command".into());
    }
    let params = arg(p, "with").cloned().unwrap_or_else(|| json!({}));
    let result = match k {
        Some(k) => {
            let r = s.part_of(cid)?.session.exec(&run, &params, Some(&mut *k))?;
            s.refresh(k).map_err(CmdError)?;
            r
        }
        None => s.part_of(cid)?.session.exec(&run, &params, None)?,
    };
    let solved = s.update()?;
    after(s, json!({ "result": result, "converged": solved.converged }))
}

static COMMANDS: &[AsmCommand] = &[
    AsmCommand {
        id: "asm.tree",
        label: "Assembly Tree",
        help: "the components (each with where it stands, whether it is mirrored and what pattern it is an element of), the relationships and the patterns and mirrors",
        mutates: false,
        kernel: false,
        run: asm_tree,
    },
    AsmCommand {
        id: "asm.geom",
        label: "Assembly Geometry",
        help: "component; and face: a face origin or {\"body\", \"index\"} | edge: [face origin, face origin] or {\"body\", \"index\"} | plane: \"xy\" | \"yz\" | \"xz\" | axis: \"x\" | \"y\" | \"z\" | origin: true | work: feature id. Without component: the assembly's own origin planes, axes and point. Returns a target for asm.constrain and asm.joint",
        mutates: false,
        kernel: false,
        run: asm_geom,
    },
    AsmCommand {
        id: "asm.constrain",
        label: "Constrain",
        help: "type: mate | flush | angle | insert; a, b: targets from asm.geom; offset (mm); angle (rad, for angle); aligned (insert: axes the same way)",
        mutates: true,
        kernel: false,
        run: asm_constrain,
    },
    AsmCommand {
        id: "asm.joint",
        label: "Joint",
        help: "type: rigid | revolute | slider | cylindrical | planar | ball; a, b: targets from asm.geom (their origins); flip (Z axes the same way); offset (mm along A's Z); angle (rad, rigid and slider)",
        mutates: true,
        kernel: false,
        run: asm_joint,
    },
    AsmCommand {
        id: "asm.edit_relationship",
        label: "Edit Relationship",
        help: "relationship; offset, angle, flip, aligned, type (joints) as in asm.constrain / asm.joint",
        mutates: true,
        kernel: false,
        run: asm_edit_relationship,
    },
    AsmCommand {
        id: "asm.suppress",
        label: "Suppress Relationship",
        help: "relationship, suppressed (default true)",
        mutates: true,
        kernel: false,
        run: asm_suppress,
    },
    AsmCommand { id: "asm.delete", label: "Delete", help: "component or relationship", mutates: true, kernel: false, run: asm_delete },
    AsmCommand {
        id: "asm.rename",
        label: "Rename",
        help: "name; component or relationship (neither: the assembly)",
        mutates: true,
        kernel: false,
        run: asm_rename,
    },
    AsmCommand {
        id: "asm.set_row",
        label: "Change Size",
        help: "component; row: a row of its part's design table (the component is the part in that size), or null for the part as its file has it",
        mutates: true,
        kernel: false,
        run: asm_set_row,
    },
    AsmCommand { id: "asm.ground", label: "Ground", help: "component, grounded (default true)", mutates: true, kernel: false, run: asm_ground },
    AsmCommand { id: "asm.visible", label: "Visibility", help: "component, visible (default true)", mutates: true, kernel: false, run: asm_visible },
    AsmCommand {
        id: "asm.move",
        label: "Free Move",
        help: "component; by: [x, y, z]; turn: {axis, angle (rad), through: a point (default its centre)}; the relationships then pull it where they must",
        mutates: true,
        kernel: false,
        run: asm_move,
    },
    AsmCommand {
        id: "asm.place",
        label: "Place At",
        help: "component; origin: [x, y, z]; z, x: its part's Z and X directions (default unturned)",
        mutates: true,
        kernel: false,
        run: asm_place,
    },
    AsmCommand { id: "asm.update", label: "Update", help: "solves again (after parts changed)", mutates: true, kernel: false, run: asm_update },
    AsmCommand {
        id: "asm.dof",
        label: "Degrees of Freedom",
        help: "each component's free directions and axes (others held still), and the assembly's total",
        mutates: false,
        kernel: false,
        run: asm_dof,
    },
    AsmCommand {
        id: "asm.bom",
        label: "Bill of Materials",
        help: "one row per part: quantity, volume, material and mass (grams, of one). A mirrored part has a row of its own, with mirrored true",
        mutates: false,
        kernel: false,
        run: asm_bom,
    },
    AsmCommand {
        id: "asm.mass",
        label: "Assembly Mass",
        help: "total volume, mass (grams) and centre of mass of the visible components, each at its part's material",
        mutates: false,
        kernel: false,
        run: asm_mass,
    },
    AsmCommand {
        id: "asm.explode.add",
        label: "Tweak Components",
        help: "components: [ids]; direction: [x, y, z] or \"x\" | \"-z\" ...; distance (mm)",
        mutates: true,
        kernel: false,
        run: asm_explode_add,
    },
    AsmCommand {
        id: "asm.explode.auto",
        label: "Auto Explode",
        help: "spacing (mm, default a quarter of the assembly's size): replaces the exploded view with one made from the relationships",
        mutates: true,
        kernel: false,
        run: asm_explode_auto,
    },
    AsmCommand {
        id: "asm.explode.clear",
        label: "Clear Explode",
        help: "step (default: every step)",
        mutates: true,
        kernel: false,
        run: asm_explode_clear,
    },
    AsmCommand {
        id: "asm.explode.positions",
        label: "Exploded Positions",
        help: "where each component is in the exploded view",
        mutates: false,
        kernel: false,
        run: asm_explode_positions,
    },
    AsmCommand { id: "asm.undo", label: "Undo", help: "", mutates: false, kernel: false, run: asm_undo },
    AsmCommand { id: "asm.redo", label: "Redo", help: "", mutates: false, kernel: false, run: asm_redo },
    AsmCommand {
        id: "asm.pattern",
        label: "Pattern Components",
        help: "components: [ids]; type: rectangular (direction: a target from asm.geom, count, spacing (mm), reverse; direction2, count2, spacing2, reverse2 for rows of rows) or circular (axis: a target, count, angle (rad, over how much of a turn they spread evenly, default all of it), reverse). One record; its elements are components held by it, which follow what they repeat",
        mutates: true,
        kernel: false,
        run: asm_pattern,
    },
    AsmCommand {
        id: "asm.mirror",
        label: "Mirror Components",
        help: "components: [ids]; plane: a target from asm.geom, a plane or flat face; reuse: those of the components, parts the same as their mirror image, to place again in the mirrored position instead of reflecting. One record; each mirrored occurrence holds its part reflected and has its own row in the parts list",
        mutates: true,
        kernel: false,
        run: asm_mirror,
    },
    AsmCommand {
        id: "asm.edit_pattern",
        label: "Edit Pattern",
        help: "pattern; what is given changes, as in asm.pattern or asm.mirror, the rest stays: components; direction, count, spacing, reverse; direction2 (null: no second direction), count2, spacing2, reverse2; axis, angle; plane, reuse. Elements are moved, added or taken away; those taken away take their relationships",
        mutates: true,
        kernel: false,
        run: asm_edit_pattern,
    },
    AsmCommand {
        id: "asm.suppress_pattern",
        label: "Suppress Pattern",
        help: "pattern, suppressed (default true): its elements are out of the assembly (not drawn, listed or checked) until it is brought back",
        mutates: true,
        kernel: false,
        run: asm_suppress_pattern,
    },
    AsmCommand {
        id: "asm.delete_pattern",
        label: "Delete Pattern",
        help: "pattern; keep_elements (default false): the elements stay where they stand, as components of their own, instead of going with it",
        mutates: true,
        kernel: false,
        run: asm_delete_pattern,
    },
    AsmCommand {
        id: "asm.interference",
        label: "Interference",
        help: "components (default all): every pair of visible components whose solids overlap, with the overlap volume",
        mutates: false,
        kernel: true,
        run: asm_interference,
    },
    AsmCommand {
        id: "asm.edit_part",
        label: "Edit Part in Place",
        help: "component; run: a part command; with: its parameters. The part changes for every component using it; the assembly is solved again",
        mutates: true,
        kernel: false,
        run: asm_edit_part,
    },
];

/// The assembly commands.
pub fn commands() -> &'static [AsmCommand] {
    COMMANDS
}

pub fn find(id: &str) -> Option<&'static AsmCommand> {
    COMMANDS.iter().find(|c| c.id == id)
}

/// Runs an assembly command. With a kernel, parts whose geometry is out of date are
/// regenerated first.
pub fn run(s: &mut AsmSession, spec: &AsmCommand, params: &Value, kernel: Option<&mut dyn Kernel>) -> CmdResult {
    run_as(s, spec, crate::schemas::schema(spec.id), params, kernel)
}

/// Runs an assembly command whose schema is `schema` (the file commands have theirs in their
/// own crate). In builds with debug assertions the command is held to it both ways
/// (`tenon_model::schema::held`).
pub fn run_as(
    s: &mut AsmSession,
    spec: &AsmCommand,
    schema: Option<&'static tenon_model::schema::CommandSchema>,
    params: &Value,
    kernel: Option<&mut dyn Kernel>,
) -> CmdResult {
    let params = if params.is_null() { &Value::Object(Default::default()) } else { params };
    if !params.is_object() {
        return Err("parameters must be a JSON object".into());
    }
    let mut kernel = kernel;
    if let Some(k) = kernel.as_deref_mut() {
        s.refresh(k).map_err(CmdError)?;
    } else if spec.kernel {
        return Err(CmdError(format!("{} needs the geometry kernel", spec.id)));
    }
    tenon_model::schema::held(spec.id, schema, params, || (spec.run)(s, kernel, params))
}

/// Runs an assembly command by id.
pub fn exec(s: &mut AsmSession, id: &str, params: &Value, kernel: Option<&mut dyn Kernel>) -> CmdResult {
    let spec = find(id).ok_or_else(|| CmdError(format!("unknown command `{id}`")))?;
    run(s, spec, params, kernel)
}
