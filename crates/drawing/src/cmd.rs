//! Drawing commands (`drw.*`): views, dimensions, tables, balloons, notes, title block fields.
//! File commands (new, open, save, placing a view of a model file, update, export) are in
//! `tenon_io::drw`.
//!
//! Sheet positions are millimetres of paper from the sheet's bottom-left corner; view positions
//! (section lines, detail circles) are millimetres of the model in the parent view.

use serde_json::{Value, json};
use tenon_assembly::ComponentId;
use tenon_geom::{Vec2, tol};
use tenon_kernel::Kernel;
use tenon_model::{CmdError, CmdResult, EdgeFingerprint, EdgeRef, FaceOrigin};

use crate::annotate::{self, format_number, from_sheet, hole_rows, parts_rows, to_sheet};
use crate::model::{AnnotId, AnnotKind, Annotation, DimKind, Drawing, GeomPick, Orientation, PickPoint, SheetId, Side, View, ViewId, ViewKind};
use crate::session::DrwSession;
use crate::views::{project, seen_size};

pub type DrwFn = fn(&mut DrwSession, Option<&mut dyn Kernel>, &Value) -> CmdResult;

/// A drawing command.
#[derive(Clone, Copy)]
pub struct DrwCommand {
    pub id: &'static str,
    pub label: &'static str,
    pub help: &'static str,
    /// Changes the drawing as one undo step.
    pub mutates: bool,
    /// Needs the views computed (a kernel, or views computed already).
    pub views: bool,
    pub run: DrwFn,
}

impl std::fmt::Debug for DrwCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DrwCommand").field("id", &self.id).finish()
    }
}

// ---- parameters -------------------------------------------------------------------------------

fn field<'v>(p: &'v Value, key: &str) -> Result<&'v Value, CmdError> {
    p.get(key).filter(|v| !v.is_null()).ok_or_else(|| CmdError(format!("missing parameter `{key}`")))
}

fn num(p: &Value, key: &str) -> Result<f64, CmdError> {
    let v = field(p, key)?.as_f64().ok_or_else(|| CmdError(format!("`{key}` must be a number")))?;
    if tol::is_valid_coord(v) { Ok(v) } else { Err(CmdError(format!("`{key}` is out of range"))) }
}

fn opt_num(p: &Value, key: &str) -> Result<Option<f64>, CmdError> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => num(p, key).map(Some),
    }
}

fn opt_bool(p: &Value, key: &str) -> Result<Option<bool>, CmdError> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_bool().map(Some).ok_or_else(|| CmdError(format!("`{key}` must be true or false"))),
    }
}

fn id(v: &Value, key: &str) -> Result<u32, CmdError> {
    v.as_u64().and_then(|n| u32::try_from(n).ok()).filter(|n| *n > 0).ok_or_else(|| CmdError(format!("`{key}` must be a positive whole number")))
}

fn view_id(p: &Value, key: &str) -> Result<ViewId, CmdError> {
    id(field(p, key)?, key).map(ViewId)
}

fn annot_id(p: &Value, key: &str) -> Result<AnnotId, CmdError> {
    id(field(p, key)?, key).map(AnnotId)
}

fn vec2(v: &Value, key: &str) -> Result<Vec2, CmdError> {
    let bad = || CmdError(format!("`{key}` must be [x, y]"));
    let out = match v {
        Value::Array(a) if a.len() == 2 => Vec2::new(a[0].as_f64().ok_or_else(bad)?, a[1].as_f64().ok_or_else(bad)?),
        Value::Object(_) => Vec2::new(v.get("x").and_then(Value::as_f64).ok_or_else(bad)?, v.get("y").and_then(Value::as_f64).ok_or_else(bad)?),
        _ => return Err(bad()),
    };
    if tol::is_valid_coord(out.x) && tol::is_valid_coord(out.y) { Ok(out) } else { Err(CmdError(format!("`{key}` is out of range"))) }
}

fn opt_vec2(p: &Value, key: &str) -> Result<Option<Vec2>, CmdError> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => vec2(v, key).map(Some),
    }
}

fn scale_param(p: &Value) -> Result<Option<f64>, CmdError> {
    let s = match p.get("scale") {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::String(t)) => {
            // "1:2" or "2:1"
            let (a, b) = t.split_once(':').ok_or("`scale` must be a number or \"a:b\"")?;
            let (a, b): (f64, f64) = (a.trim().parse().map_err(|_| "bad scale")?, b.trim().parse().map_err(|_| "bad scale")?);
            a / b
        }
        Some(v) => v.as_f64().ok_or("`scale` must be a number or \"a:b\"")?,
    };
    if s.is_finite() && (1e-4..=1e4).contains(&s) { Ok(Some(s)) } else { Err("`scale` is out of range".into()) }
}

fn pick(p: &Value, key: &str) -> Result<GeomPick, CmdError> {
    serde_json::from_value(field(p, key)?.clone()).map_err(|e| CmdError(format!("`{key}` is not a pick (make one with drw.pick): {e}")))
}

fn opt_pick(p: &Value, key: &str) -> Result<Option<GeomPick>, CmdError> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => pick(p, key).map(Some),
    }
}

fn v2(v: Vec2) -> Value {
    json!([v.x, v.y])
}

fn view(d: &Drawing, id: ViewId) -> Result<&View, CmdError> {
    d.view(id).ok_or_else(|| CmdError(format!("{id} does not exist")))
}

// ---- reading ----------------------------------------------------------------------------------

/// Everything about the drawing, with dimension values and hole table rows as they are now.
fn drw_info(s: &mut DrwSession, _k: Option<&mut dyn Kernel>, _p: &Value) -> CmdResult {
    let d = s.drawing();
    let ev = s.evaluation();
    let sheets: Vec<Value> = d
        .sheets
        .iter()
        .map(|sh| json!({ "id": sh.id.0, "name": sh.name, "size": sh.size.name, "width": sh.size.width, "height": sh.size.height }))
        .collect();
    let views: Vec<Value> = d
        .views
        .iter()
        .map(|v| {
            let g = ev.and_then(|e| e.view(v.id));
            let count = |vis: bool| g.map_or(0, |g| g.curves.iter().filter(|c| c.visible == vis).count());
            let size = g.and_then(|g| g.bounds).map(|(a, b)| v2((b - a) * v.scale));
            json!({
                "id": v.id.0, "name": v.name, "sheet": v.sheet.0, "model": v.model,
                "kind": serde_json::to_value(&v.kind).unwrap_or(Value::Null),
                "scale": v.scale, "center": v2(v.center), "size": size,
                "direction": g.map(|g| { let z = g.frame.z(); json!([z.x, z.y, z.z]) }),
                "visible_curves": count(true), "hidden_curves": count(false), "hatch": g.map_or(0, |g| g.hatch.len()),
                "error": g.and_then(|g| g.error.clone()),
            })
        })
        .collect();
    let annotations: Vec<Value> = d
        .annotations
        .iter()
        .map(|a| {
            let mut v = serde_json::to_value(&a.kind).unwrap_or(Value::Null);
            if let Some(o) = v.as_object_mut() {
                o.insert("id".into(), json!(a.id.0));
                match (&a.kind, ev) {
                    (AnnotKind::Dimension { .. }, Some(e)) => match annotate::dimension(d, e, a) {
                        Ok((value, text)) => {
                            o.insert("value".into(), json!(value));
                            o.insert("shown".into(), json!(text));
                        }
                        Err(err) => {
                            o.insert("problem".into(), json!(err));
                        }
                    },
                    (AnnotKind::HoleTable { view, origin, .. }, Some(e)) => match hole_rows(d, e, *view, origin.as_ref()) {
                        Ok(rows) => {
                            let rows: Vec<Value> =
                                rows.iter().map(|r| json!({ "tag": r.tag, "x": r.x, "y": r.y, "description": r.description })).collect();
                            o.insert("rows".into(), json!(rows));
                        }
                        Err(err) => {
                            o.insert("problem".into(), json!(err));
                        }
                    },
                    (AnnotKind::PartsList { view, .. }, Some(e)) => {
                        if let Some(m) = d.view(*view).and_then(|v| e.models.get(&v.model)) {
                            let rows: Vec<Value> = parts_rows(m)
                                .iter()
                                .map(
                                    |r| json!({ "item": r.item, "quantity": r.quantity, "part_number": r.part_number, "description": r.description }),
                                )
                                .collect();
                            o.insert("rows".into(), json!(rows));
                        }
                    }
                    (AnnotKind::CenterMark { .. } | AnnotKind::Centerline { .. } | AnnotKind::CenterlineBisector { .. }, Some(e)) => {
                        match annotate::center_segments(d, e, a) {
                            Ok(segs) => {
                                let segs: Vec<Value> = segs.iter().map(|[p, q]| json!([v2(*p), v2(*q)])).collect();
                                o.insert("segments".into(), json!(segs));
                            }
                            Err(err) => {
                                o.insert("problem".into(), json!(err));
                            }
                        }
                    }
                    (AnnotKind::Balloon { view, component, .. }, Some(e)) => {
                        if let Some(m) = d.view(*view).and_then(|v| e.models.get(&v.model)) {
                            o.insert("item".into(), json!(annotate::item_of(m, *component)));
                        }
                    }
                    _ => {}
                }
            }
            v
        })
        .collect();
    Ok(
        json!({ "name": d.name, "standard": d.standard, "props": d.props, "sheets": sheets, "views": views, "annotations": annotations, "computed": ev.is_some() }),
    )
}

/// How near a sheet point an edge must be drawn for `drw.pick` with `at` to find it (sheet mm).
pub const PICK_REACH: f64 = 2.5;

/// A pick on an edge of a view's model: the edge drawn nearest a sheet point (as a click picks
/// it), or one named by its two faces (as `asm.geom` names edges) or by index.
fn drw_pick(s: &mut DrwSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let vid = view_id(p, "view")?;
    let d = s.drawing();
    let v = view(d, vid)?;
    let ev = s.current()?;
    let m = ev.models.get(&v.model).ok_or("the model is not loaded")?;
    let point = match p.get("point").and_then(Value::as_str) {
        Some(t) => PickPoint::from_id(t).ok_or("`point` must be start, end, mid, center or whole")?,
        None => PickPoint::Whole,
    };
    let at = match (opt_vec2(p, "at")?, opt_vec2(p, "view_at")?) {
        (Some(at), _) => Some(at),
        (None, Some(q)) => Some(to_sheet(v, ev.view(vid).ok_or("the view is not computed")?, q)),
        (None, None) => None,
    };
    if let Some(at) = at {
        let hit = annotate::edge_at(d, ev, vid, at, PICK_REACH).ok_or("no edge is drawn there")?;
        return pick_result(d, ev, vid, GeomPick { component: hit.component, edge: hit.edge, point });
    }
    let component = match p.get("component") {
        None | Some(Value::Null) => None,
        Some(c) => Some(ComponentId(id(c, "component")?)),
    };
    let inst = m.instance(component).ok_or("give `component` for an assembly view")?;
    let scene = m.scenes.get(&inst.part).ok_or("the part's geometry is not available")?;
    let e = field(p, "edge").map_err(|_| CmdError("give `at` (a sheet point) or `edge`".into()))?;
    let (b, ei) = if let Some(faces) = e.as_array() {
        let names: Vec<FaceOrigin> = faces
            .iter()
            .map(|f| serde_json::from_value(f.clone()))
            .collect::<Result<_, _>>()
            .map_err(|e| CmdError(format!("`edge` must be two face origins: {e}")))?;
        let [x, y] = names.as_slice() else { return Err("`edge` must be two face origins".into()) };
        let want = EdgeRef::new(*x, *y, EdgeFingerprint { mid: tenon_geom::Vec3::ZERO, length: 0.0 }).faces;
        let mut found = scene
            .bodies
            .iter()
            .enumerate()
            .flat_map(|(bi, b)| b.edges.iter().enumerate().filter(move |(_, (n, _))| *n == Some(want)).map(move |(ei, _)| (bi, ei)));
        match (found.next(), found.next()) {
            (Some(x), None) => x,
            (None, _) => return Err("no edge joins those faces".into()),
            (Some(_), Some(_)) => return Err("several edges join those faces; give `edge` as {\"body\": b, \"index\": i}".into()),
        }
    } else {
        (
            e.get("body").and_then(Value::as_u64).unwrap_or(0) as usize,
            e.get("index").and_then(Value::as_u64).ok_or("`edge` needs two face origins or an index")? as usize,
        )
    };
    let body = scene.bodies.get(b).ok_or("no such body")?;
    let (names, fp) = body.edges.get(ei).ok_or("no such edge")?;
    let [x, y] = names.ok_or("this edge cannot be referenced yet")?;
    pick_result(d, ev, vid, GeomPick { component, edge: EdgeRef::new(x, y, fp.clone()), point })
}

/// A pick as `drw.pick` returns it: the pick, what it is (a point, a line and its length in the
/// view, a circle and its diameter) and where it is on the sheet.
fn pick_result(d: &Drawing, ev: &crate::views::Evaluation, vid: ViewId, gp: GeomPick) -> CmdResult {
    let v = view(d, vid)?;
    let m = ev.models.get(&v.model).ok_or("the model is not loaded")?;
    let r = annotate::resolve_pick(m, &gp).map_err(CmdError)?;
    let mut out = serde_json::to_value(&gp).map_err(|e| CmdError(e.to_string()))?;
    if let (Some(o), Some(g)) = (out.as_object_mut(), ev.view(vid)) {
        let f = &g.frame;
        let at = match r {
            annotate::Resolved::Point(q) => {
                o.insert("kind".into(), json!("point"));
                project(f, q)
            }
            annotate::Resolved::Circle { center, radius, .. } => {
                o.insert("kind".into(), json!("circle"));
                o.insert("diameter".into(), json!(2.0 * radius));
                project(f, center)
            }
            annotate::Resolved::Line(a, b) => {
                o.insert("kind".into(), json!("line"));
                o.insert("length".into(), json!(project(f, a).dist(project(f, b))));
                (project(f, a) + project(f, b)) * 0.5
            }
        };
        o.insert("at".into(), v2(to_sheet(v, g, at)));
    }
    Ok(out)
}

// ---- views ------------------------------------------------------------------------------------

/// The standard scales, largest first.
const SCALES: [f64; 13] = [10.0, 5.0, 4.0, 2.0, 1.0, 0.5, 0.25, 0.2, 0.1, 0.05, 0.02, 0.01, 0.005];

/// The largest standard scale that fits a model of `size` (model mm, in the view) into a third
/// of the sheet across and two fifths up.
pub fn fitting_scale(size: Vec2, sheet: Vec2) -> f64 {
    SCALES.into_iter().find(|s| size.x * s <= sheet.x * 0.33 && size.y * s <= sheet.y * 0.4).unwrap_or(0.005)
}

/// Places a base view of a model already loaded in the session (`model`: its key). Used by
/// `drw.view.base` in `tenon_io::drw`, which loads the file first.
pub fn add_base_view(s: &mut DrwSession, model: &str, p: &Value) -> CmdResult {
    let orientation = match p.get("orientation").and_then(Value::as_str) {
        Some(o) => Orientation::from_id(o).ok_or("`orientation` must be front, back, top, bottom, left, right or iso")?,
        None => Orientation::Front,
    };
    let sheet = match p.get("sheet") {
        None | Some(Value::Null) => s.drawing().sheets[0].id,
        Some(v) => SheetId(id(v, "sheet")?),
    };
    let sh = s.drawing().sheet(sheet).ok_or_else(|| CmdError(format!("{sheet} does not exist")))?.clone();
    let f = crate::views::base_frame(orientation);
    let size = s.last_evaluation().and_then(|e| e.models.get(model)).and_then(|m| seen_size(m, &f));
    let scale = match scale_param(p)? {
        Some(sc) => sc,
        None => size.map_or(1.0, |z| fitting_scale(z, Vec2::new(sh.size.width, sh.size.height))),
    };
    let at = opt_vec2(p, "at")?.unwrap_or(Vec2::new(sh.size.width * 0.28, sh.size.height * 0.62));
    let hidden = opt_bool(p, "hidden")?.unwrap_or(orientation != Orientation::Iso);
    let model = model.to_owned();
    let vid = s.edit(|d| {
        let id = d.take_view_id();
        let name = d.next_view_name(false);
        d.views.push(View {
            id,
            sheet,
            name,
            model,
            kind: ViewKind::Base { orientation },
            scale,
            center: at,
            hidden,
            tangent: false,
            centerlines: true,
            label: false,
        });
        Ok(id)
    })?;
    let name = view(s.drawing(), vid)?.name.clone();
    Ok(json!({ "view": vid.0, "name": name, "scale": scale }))
}

/// The gap left between a view and a new view placed beside it (sheet mm).
const VIEW_GAP: f64 = 20.0;

/// Half a view's size on the sheet.
fn half_size(s: &DrwSession, v: &View) -> Vec2 {
    s.last_evaluation().and_then(|e| e.view(v.id)).and_then(|g| g.bounds).map_or(Vec2::new(30.0, 30.0), |(a, b)| (b - a) * (0.5 * v.scale))
}

/// Where a new view of the parent's model, seen in frame `f` at `scale`, goes on the sheet: off
/// the parent towards `dir`, clear of it by [`VIEW_GAP`].
fn beside(s: &DrwSession, parent: &View, f: &tenon_geom::Frame, scale: f64, dir: Vec2) -> Vec2 {
    let ph = half_size(s, parent);
    let ch = s
        .last_evaluation()
        .and_then(|e| e.models.get(&parent.model))
        .and_then(|m| seen_size(m, f))
        .map_or(Vec2::new(30.0, 30.0), |z| z * (0.5 * scale));
    parent.center + Vec2::new(dir.x * (ph.x + ch.x + VIEW_GAP), dir.y * (ph.y + ch.y + VIEW_GAP))
}

fn frame_of(s: &DrwSession, v: ViewId) -> tenon_geom::Frame {
    crate::views::frames(s.drawing()).get(&v).copied().unwrap_or(tenon_geom::Frame::WORLD)
}

fn drw_projected(s: &mut DrwSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let parent = view_id(p, "parent")?;
    let pv = view(s.drawing(), parent)?.clone();
    let side =
        Side::from_id(field(p, "side")?.as_str().unwrap_or("")).ok_or("`side` must be right, left, above, below or a diagonal like above_right")?;
    let scale = scale_param(p)?.unwrap_or(pv.scale);
    let at = match opt_vec2(p, "at")? {
        Some(a) => a,
        None => {
            let f = crate::views::projected_frame(&frame_of(s, parent), side, s.drawing().standard == crate::model::Standard::Ansi);
            beside(s, &pv, &f, scale, side.step())
        }
    };
    let hidden = opt_bool(p, "hidden")?.unwrap_or(!side.diagonal() && pv.hidden);
    let vid = s.edit(|d| {
        let id = d.take_view_id();
        let name = d.next_view_name(false);
        d.views.push(View {
            id,
            sheet: pv.sheet,
            name,
            model: pv.model.clone(),
            kind: ViewKind::Projected { parent, side },
            scale,
            center: at,
            hidden,
            tangent: false,
            centerlines: !side.diagonal(),
            label: false,
        });
        Ok(id)
    })?;
    let name = view(s.drawing(), vid)?.name.clone();
    Ok(json!({ "view": vid.0, "name": name }))
}

fn drw_section(s: &mut DrwSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let parent = view_id(p, "parent")?;
    let pv = view(s.drawing(), parent)?.clone();
    let (a, b) = (vec2(field(p, "a")?, "a")?, vec2(field(p, "b")?, "b")?);
    if a.dist(b) < tol::LINEAR {
        return Err("the section line needs two different points".into());
    }
    let flip = opt_bool(p, "flip")?.unwrap_or(false);
    // Third-angle: the section goes on the side it is seen from, against the arrows.
    let d = (b - a).normalized();
    let look = if flip { Vec2::new(-d.y, d.x) } else { Vec2::new(d.y, -d.x) };
    let toward = if s.drawing().standard == crate::model::Standard::Ansi { -look } else { look };
    let scale = scale_param(p)?.unwrap_or(pv.scale);
    let at = match opt_vec2(p, "at")? {
        Some(a) => a,
        None => {
            let (f, _, _) = crate::views::section_frame(&frame_of(s, parent), a, b, flip);
            beside(s, &pv, &f, scale, toward)
        }
    };
    let vid = s.edit(|d| {
        let id = d.take_view_id();
        let name = d.next_view_name(true);
        d.views.push(View {
            id,
            sheet: pv.sheet,
            name,
            model: pv.model.clone(),
            kind: ViewKind::Section { parent, a, b, flip },
            scale,
            center: at,
            hidden: false,
            tangent: false,
            centerlines: true,
            label: true,
        });
        Ok(id)
    })?;
    let name = view(s.drawing(), vid)?.name.clone();
    Ok(json!({ "view": vid.0, "name": name }))
}

fn drw_detail(s: &mut DrwSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let parent = view_id(p, "parent")?;
    let pv = view(s.drawing(), parent)?.clone();
    let center = vec2(field(p, "center")?, "center")?;
    let radius = num(p, "radius")?;
    if radius.is_nan() || radius <= tol::LINEAR {
        return Err("`radius` must be positive".into());
    }
    let scale = scale_param(p)?.unwrap_or(pv.scale * 2.0);
    let at = match opt_vec2(p, "at")? {
        Some(a) => a,
        None => pv.center + Vec2::new(half_size(s, &pv).x + VIEW_GAP + radius * scale, 0.0),
    };
    let vid = s.edit(|d| {
        let id = d.take_view_id();
        let name = d.next_view_name(true);
        d.views.push(View {
            id,
            sheet: pv.sheet,
            name,
            model: pv.model.clone(),
            kind: ViewKind::Detail { parent, center, radius },
            scale,
            center: at,
            hidden: pv.hidden,
            tangent: false,
            centerlines: false,
            label: true,
        });
        Ok(id)
    })?;
    let name = view(s.drawing(), vid)?.name.clone();
    Ok(json!({ "view": vid.0, "name": name }))
}

fn drw_view_edit(s: &mut DrwSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let vid = view_id(p, "view")?;
    let (scale, at, by) = (scale_param(p)?, opt_vec2(p, "at")?, opt_vec2(p, "by")?);
    let (hidden, tangent, centerlines, label) = (opt_bool(p, "hidden")?, opt_bool(p, "tangent")?, opt_bool(p, "centerlines")?, opt_bool(p, "label")?);
    s.edit(|d| {
        let v = d.view(vid).ok_or_else(|| CmdError(format!("{vid} does not exist")))?;
        let old = v.center;
        let target = match (at, by) {
            (Some(a), _) => a,
            (None, Some(b)) => old + b,
            (None, None) => old,
        };
        let mut shift = target - old;
        // A view projected beside its parent stays lined up with it: it only slides along.
        match &v.kind {
            ViewKind::Projected { side: Side::Right | Side::Left, .. } => shift.y = 0.0,
            ViewKind::Projected { side: Side::Above | Side::Below, .. } => shift.x = 0.0,
            _ => {}
        }
        // Projected views move with their parent (they stay lined up with it).
        let family: Vec<ViewId> =
            d.family(vid).into_iter().filter(|f| *f == vid || matches!(d.view(*f).map(|v| &v.kind), Some(ViewKind::Projected { .. }))).collect();
        for f in family {
            if let Some(v) = d.view_mut(f) {
                v.center += shift;
            }
        }
        let v = d.view_mut(vid).ok_or("gone")?;
        if let Some(sc) = scale {
            v.scale = sc;
        }
        v.hidden = hidden.unwrap_or(v.hidden);
        v.tangent = tangent.unwrap_or(v.tangent);
        v.centerlines = centerlines.unwrap_or(v.centerlines);
        v.label = label.unwrap_or(v.label);
        Ok(())
    })?;
    let v = view(s.drawing(), vid)?;
    Ok(json!({ "view": vid.0, "center": v2(v.center), "scale": v.scale }))
}

fn drw_view_delete(s: &mut DrwSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let vid = view_id(p, "view")?;
    s.edit(|d| if d.remove_view(vid) { Ok(()) } else { Err(CmdError(format!("{vid} does not exist"))) })?;
    Ok(json!({ "deleted": vid.0 }))
}

// ---- annotations ------------------------------------------------------------------------------

fn add_annotation(s: &mut DrwSession, kind: AnnotKind) -> Result<AnnotId, CmdError> {
    s.edit(|d| {
        let id = d.take_annotation_id();
        d.annotations.push(Annotation { id, kind });
        Ok(id)
    })
}

fn drw_dimension(s: &mut DrwSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let vid = view_id(p, "view")?;
    let v = view(s.drawing(), vid)?.clone();
    let dim = DimKind::from_id(field(p, "type")?.as_str().unwrap_or(""))
        .ok_or("`type` must be horizontal, vertical, aligned, diameter, radius or angle")?;
    let a = pick(p, "a")?;
    let b = opt_pick(p, "b")?;
    let at = match opt_vec2(p, "at")? {
        Some(at) => at,
        None => {
            let by = opt_vec2(p, "by")?.ok_or("give `at` (a sheet point) or `by` (from the middle of what is measured)")?;
            let ev = s.current()?;
            let g = ev.view(vid).ok_or("the view is not computed")?;
            let m = ev.models.get(&v.model).ok_or("the model is not loaded")?;
            annotate::dimension_anchor(&v, g, m, dim, &a, b.as_ref()).map_err(CmdError)? + by
        }
    };
    let text = p.get("text").and_then(Value::as_str).map(str::to_owned);
    let precision = match p.get("precision") {
        None | Some(Value::Null) => 2,
        Some(x) => x.as_u64().and_then(|n| u8::try_from(n).ok()).filter(|n| *n <= 6).ok_or("`precision` must be 0 to 6")?,
    };
    let kind = AnnotKind::Dimension { view: vid, dim, a, b, offset: at - v.center, text, precision };
    // Refuse what cannot be measured.
    let probe = Annotation { id: AnnotId(u32::MAX), kind: kind.clone() };
    let (value, shown) = annotate::dimension(s.drawing(), s.current()?, &probe).map_err(CmdError)?;
    let aid = add_annotation(s, kind)?;
    Ok(json!({ "annotation": aid.0, "value": value, "shown": shown }))
}

fn suggestion_json(s: &crate::suggest::Suggestion) -> Value {
    json!({
        "type": s.dim, "a": s.a, "b": s.b, "at": v2(s.at), "text": s.text,
        "value": s.value, "shown": s.shown, "why": s.why,
    })
}

/// The dimensions suggested for a view (nothing is added).
fn drw_dimension_suggest(s: &mut DrwSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let vid = view_id(p, "view")?;
    let list = crate::suggest::suggest(s.drawing(), s.current()?, vid).map_err(CmdError)?;
    Ok(json!({ "view": vid.0, "suggestions": list.iter().map(suggestion_json).collect::<Vec<_>>() }))
}

/// Adds suggested dimensions to a view (all, or those listed in `accept`), as one undo step.
fn drw_dimension_auto(s: &mut DrwSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let vid = view_id(p, "view")?;
    let v = view(s.drawing(), vid)?.clone();
    let list = crate::suggest::suggest(s.drawing(), s.current()?, vid).map_err(CmdError)?;
    let chosen: Vec<usize> = match p.get("accept") {
        None | Some(Value::Null) => (0..list.len()).collect(),
        Some(Value::Array(a)) => a
            .iter()
            .map(|x| {
                x.as_u64()
                    .map(|i| i as usize)
                    .filter(|i| *i < list.len())
                    .ok_or_else(|| CmdError(format!("`accept` holds {x}, not one of the {} suggestions", list.len())))
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err("`accept` must be a list of suggestion numbers".into()),
    };
    let kinds: Vec<AnnotKind> = chosen.iter().filter_map(|i| list.get(*i)).map(|sg| sg.kind(vid, &v)).collect();
    let ids = s.edit(|d| {
        Ok(kinds
            .into_iter()
            .map(|kind| {
                let id = d.take_annotation_id();
                d.annotations.push(Annotation { id, kind });
                id.0
            })
            .collect::<Vec<_>>())
    })?;
    let added: Vec<Value> = chosen.iter().filter_map(|i| list.get(*i)).map(suggestion_json).collect();
    Ok(json!({ "added": ids.len(), "annotations": ids, "dimensions": added }))
}

/// Adds a centre mark or line after checking it can be drawn; returns it and its segments.
fn add_center(s: &mut DrwSession, kind: AnnotKind) -> CmdResult {
    let probe = Annotation { id: AnnotId(u32::MAX), kind: kind.clone() };
    let segments = annotate::center_segments(s.drawing(), s.current()?, &probe).map_err(CmdError)?;
    let aid = add_annotation(s, kind)?;
    let segments: Vec<Value> = segments.iter().map(|[a, b]| json!([v2(*a), v2(*b)])).collect();
    Ok(json!({ "annotation": aid.0, "segments": segments }))
}

fn drw_center_mark(s: &mut DrwSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let vid = view_id(p, "view")?;
    view(s.drawing(), vid)?;
    add_center(s, AnnotKind::CenterMark { view: vid, a: pick(p, "a")? })
}

fn drw_centerline(s: &mut DrwSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let vid = view_id(p, "view")?;
    view(s.drawing(), vid)?;
    add_center(s, AnnotKind::Centerline { view: vid, a: pick(p, "a")?, b: pick(p, "b")? })
}

fn drw_centerline_bisector(s: &mut DrwSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let vid = view_id(p, "view")?;
    view(s.drawing(), vid)?;
    add_center(s, AnnotKind::CenterlineBisector { view: vid, a: pick(p, "a")?, b: pick(p, "b")? })
}

fn drw_hole_table(s: &mut DrwSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let vid = view_id(p, "view")?;
    let origin = opt_pick(p, "origin")?;
    let rows = hole_rows(s.drawing(), s.current()?, vid, origin.as_ref()).map_err(CmdError)?;
    if rows.is_empty() {
        return Err("the view shows no holes end-on".into());
    }
    let sheet = view(s.drawing(), vid)?.sheet;
    let sh = s.drawing().sheet(sheet).ok_or("no sheet")?.clone();
    let (w, h) = annotate::hole_table_size(rows.len());
    // Default: top right, inside the border.
    let at = opt_vec2(p, "at")?.unwrap_or(Vec2::new(sh.size.width - crate::sheets::BORDER - 5.0 - w, sh.size.height - crate::sheets::BORDER - 5.0));
    let _ = h;
    let aid = add_annotation(s, AnnotKind::HoleTable { view: vid, at, origin })?;
    let out: Vec<Value> = rows.iter().map(|r| json!({ "tag": r.tag, "x": r.x, "y": r.y, "description": r.description })).collect();
    Ok(json!({ "annotation": aid.0, "rows": out }))
}

fn drw_parts_list(s: &mut DrwSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let vid = view_id(p, "view")?;
    let v = view(s.drawing(), vid)?.clone();
    let m = s.current()?.models.get(&v.model).ok_or("the model is not loaded")?;
    if m.assembly.is_none() {
        return Err("a parts list needs an assembly view".into());
    }
    let rows = parts_rows(m);
    let sh = s.drawing().sheet(v.sheet).ok_or("no sheet")?.clone();
    let (w, h) = annotate::parts_list_size(rows.len());
    // Default: just above the title block, at the right.
    let at = opt_vec2(p, "at")?.unwrap_or(Vec2::new(sh.size.width - crate::sheets::BORDER - w, crate::sheets::BORDER + 40.0 + 4.0 + h));
    let aid = add_annotation(s, AnnotKind::PartsList { view: vid, at })?;
    let out: Vec<Value> =
        rows.iter().map(|r| json!({ "item": r.item, "quantity": r.quantity, "part_number": r.part_number, "description": r.description })).collect();
    Ok(json!({ "annotation": aid.0, "rows": out }))
}

/// Where a component's balloon attaches (part coordinates) and where the balloon goes on the
/// sheet: out from the view's middle, past its edge. It attaches to an edge of the component, at
/// the piece of edge nearest the eye (so on what is seen of it).
fn balloon_place(s: &DrwSession, v: &View, component: ComponentId) -> Result<(tenon_geom::Vec3, Vec2), CmdError> {
    let ev = s.current()?;
    let m = ev.models.get(&v.model).ok_or("the model is not loaded")?;
    let g = ev.view(v.id).ok_or("the view is not computed")?;
    let inst = m.instance(Some(component)).ok_or("the component is not in the view's assembly")?;
    let scene = m.scenes.get(&inst.part).ok_or("the part's geometry is not available")?;
    let p3 = |p: &[f32; 3]| tenon_geom::Vec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
    let depth = |p: tenon_geom::Vec3| inst.frame.to_world(p).dot(g.frame.z());
    let attach = scene
        .bodies
        .iter()
        .flat_map(|b| b.mesh.edges.iter())
        .flat_map(|e| e.points.windows(2).map(|w| (p3(&w[0]) + p3(&w[1])) * 0.5))
        .max_by(|a, b| depth(*a).total_cmp(&depth(*b)))
        .or_else(|| scene.bbox().map(|b| b.center()))
        .unwrap_or(tenon_geom::Vec3::ZERO);
    let p = to_sheet(v, g, project(&g.frame, inst.frame.to_world(attach)));
    let (lo, hi) = g.bounds.map_or((p, p), |(a, b)| (to_sheet(v, g, a), to_sheet(v, g, b)));
    let mid = (lo + hi) * 0.5;
    let dir = (p - mid).normalized();
    let dir = if dir.is_finite() && dir.len() > 0.5 { dir } else { Vec2::new(1.0, 1.0).normalized() };
    // Out to the box around the view, then 12 mm more.
    let half = (hi - lo) * 0.5;
    let t = [half.x / dir.x.abs().max(1e-9), half.y / dir.y.abs().max(1e-9)].into_iter().fold(f64::MAX, f64::min);
    Ok((attach, mid + dir * (t + 12.0)))
}

fn drw_balloon(s: &mut DrwSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let vid = view_id(p, "view")?;
    let v = view(s.drawing(), vid)?.clone();
    // Attached where an edge of the component was clicked, or on its own.
    let hit = match opt_vec2(p, "attach_at")? {
        Some(q) => Some(annotate::edge_at(s.drawing(), s.current()?, vid, q, PICK_REACH).ok_or("no edge is drawn where the balloon attaches")?),
        None => None,
    };
    let component = match (p.get("component").filter(|c| !c.is_null()), &hit) {
        (Some(c), _) => ComponentId(id(c, "component")?),
        (None, Some(h)) => h.component.ok_or("balloons are for assembly views")?,
        (None, None) => return Err("give `component` or `attach_at`".into()),
    };
    let (auto_attach, auto) = balloon_place(s, &v, component)?;
    let attach = match &hit {
        Some(h) if h.component == Some(component) => h.local,
        _ => auto_attach,
    };
    let at = opt_vec2(p, "at")?.unwrap_or(auto);
    let m = s.current()?.models.get(&v.model).ok_or("the model is not loaded")?;
    let item = annotate::item_of(m, component).ok_or("the component is not in the parts list")?;
    let aid = add_annotation(s, AnnotKind::Balloon { view: vid, component, attach, offset: at - v.center })?;
    Ok(json!({ "annotation": aid.0, "item": item }))
}

/// A balloon for each part of an assembly view (on its first component), around the view.
fn drw_balloon_auto(s: &mut DrwSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let vid = view_id(p, "view")?;
    let v = view(s.drawing(), vid)?.clone();
    let m = s.current()?.models.get(&v.model).ok_or("the model is not loaded")?.clone();
    let asm = m.assembly.as_ref().ok_or("balloons need an assembly view")?;
    let have: Vec<ComponentId> = s
        .drawing()
        .annotations
        .iter()
        .filter_map(|a| match a.kind {
            AnnotKind::Balloon { view, component, .. } if view == vid => Some(component),
            _ => None,
        })
        .collect();
    let mut kinds = Vec::new();
    for row in parts_rows(&m) {
        let Some(c) = asm.components.iter().find(|c| c.part == row.part && c.visible) else { continue };
        let ballooned = have.iter().any(|h| asm.component(*h).is_some_and(|x| x.part == row.part));
        if !ballooned {
            let (attach, at) = balloon_place(s, &v, c.id)?;
            kinds.push(AnnotKind::Balloon { view: vid, component: c.id, attach, offset: at - v.center });
        }
    }
    let n = kinds.len();
    s.edit(|d| {
        for kind in kinds {
            let id = d.take_annotation_id();
            d.annotations.push(Annotation { id, kind });
        }
        Ok(())
    })?;
    Ok(json!({ "added": n }))
}

fn drw_note(s: &mut DrwSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let text = field(p, "text")?.as_str().ok_or("`text` must be text")?.to_owned();
    if text.is_empty() || text.len() > 10_000 {
        return Err("`text` must be 1 to 10000 characters".into());
    }
    let at = vec2(field(p, "at")?, "at")?;
    let sheet = match p.get("sheet") {
        None | Some(Value::Null) => s.drawing().sheets[0].id,
        Some(v) => SheetId(id(v, "sheet")?),
    };
    let height = opt_num(p, "height")?.unwrap_or(3.5);
    let aid = add_annotation(s, AnnotKind::Note { sheet, at, text, height })?;
    Ok(json!({ "annotation": aid.0 }))
}

/// Moves an annotation by `by` (sheet mm), or changes a dimension's text or precision.
fn drw_annotation_edit(s: &mut DrwSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let aid = annot_id(p, "annotation")?;
    let by = opt_vec2(p, "by")?.unwrap_or(Vec2::new(0.0, 0.0));
    let text = p.get("text").map(|t| t.as_str().map(str::to_owned));
    let precision = match p.get("precision") {
        None | Some(Value::Null) => None,
        Some(x) => Some(x.as_u64().and_then(|n| u8::try_from(n).ok()).filter(|n| *n <= 6).ok_or("`precision` must be 0 to 6")?),
    };
    s.edit(|d| {
        let a = d.annotations.iter_mut().find(|a| a.id == aid).ok_or_else(|| CmdError(format!("{aid} does not exist")))?;
        match &mut a.kind {
            AnnotKind::Dimension { offset, text: t, precision: pr, .. } => {
                *offset += by;
                if let Some(x) = text {
                    *t = x;
                }
                if let Some(x) = precision {
                    *pr = x;
                }
            }
            AnnotKind::Balloon { offset, .. } => *offset += by,
            AnnotKind::CenterMark { .. } | AnnotKind::Centerline { .. } | AnnotKind::CenterlineBisector { .. } => {
                if by.len() > 0.0 {
                    return Err("a centre mark or line sits on its geometry and moves with it, not by itself".into());
                }
            }
            AnnotKind::HoleTable { at, .. } | AnnotKind::PartsList { at, .. } => *at += by,
            AnnotKind::Note { at, text: t, .. } => {
                *at += by;
                if let Some(Some(x)) = text {
                    *t = x;
                }
            }
        }
        Ok(())
    })?;
    Ok(json!({ "annotation": aid.0 }))
}

fn drw_delete(s: &mut DrwSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let aid = annot_id(p, "annotation")?;
    s.edit(|d| {
        let before = d.annotations.len();
        d.annotations.retain(|a| a.id != aid);
        if d.annotations.len() == before { Err(CmdError(format!("{aid} does not exist"))) } else { Ok(()) }
    })?;
    Ok(json!({ "deleted": aid.0 }))
}

fn drw_props(s: &mut DrwSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let get = |k: &str| -> Result<Option<String>, CmdError> {
        match p.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => v
                .as_str()
                .filter(|s| s.len() <= 256)
                .map(|s| Some(s.to_owned()))
                .ok_or_else(|| CmdError(format!("`{k}` must be text of at most 256 characters"))),
        }
    };
    let (title, number, revision, company, drawn_by, date) =
        (get("title")?, get("number")?, get("revision")?, get("company")?, get("drawn_by")?, get("date")?);
    s.edit(|d| {
        let pr = &mut d.props;
        for (slot, v) in [
            (&mut pr.title, title),
            (&mut pr.number, number),
            (&mut pr.revision, revision),
            (&mut pr.company, company),
            (&mut pr.drawn_by, drawn_by),
            (&mut pr.date, date),
        ] {
            if let Some(v) = v {
                *slot = v;
            }
        }
        Ok(())
    })?;
    Ok(serde_json::to_value(&s.drawing().props).unwrap_or(Value::Null))
}

fn drw_sheet_add(s: &mut DrwSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let size = match p.get("size").and_then(Value::as_str) {
        Some(n) => crate::sheets::size_named(n).ok_or("unknown sheet size (A, B, C, D, A4, A3, A2, A1)")?,
        None => crate::sheets::default_size(s.drawing().standard),
    };
    let id = s.edit(|d| Ok(d.add_sheet(size)))?;
    Ok(json!({ "sheet": id.0 }))
}

fn drw_sheet_size(s: &mut DrwSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let sheet = SheetId(id(field(p, "sheet")?, "sheet")?);
    let size = crate::sheets::size_named(field(p, "size")?.as_str().unwrap_or("")).ok_or("unknown sheet size (A, B, C, D, A4, A3, A2, A1)")?;
    s.edit(|d| {
        let standard = d.standard;
        let sh = d.sheets.iter_mut().find(|x| x.id == sheet).ok_or_else(|| CmdError(format!("{sheet} does not exist")))?;
        sh.title_block = crate::sheets::title_block(standard, &size);
        sh.size = size;
        Ok(())
    })?;
    Ok(json!({ "sheet": sheet.0 }))
}

fn drw_undo(s: &mut DrwSession, _k: Option<&mut dyn Kernel>, _p: &Value) -> CmdResult {
    if !s.undo() {
        return Err("nothing to undo".into());
    }
    Ok(json!({ "revision": s.revision() }))
}

fn drw_redo(s: &mut DrwSession, _k: Option<&mut dyn Kernel>, _p: &Value) -> CmdResult {
    if !s.redo() {
        return Err("nothing to redo".into());
    }
    Ok(json!({ "revision": s.revision() }))
}

/// Where a sheet point is in a view (model mm), for placing section lines and detail circles.
fn drw_to_view(s: &mut DrwSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let vid = view_id(p, "view")?;
    let v = view(s.drawing(), vid)?;
    let g = s.current()?.view(vid).ok_or("the view is not computed")?;
    let at = vec2(field(p, "at")?, "at")?;
    let q = from_sheet(v, g, at);
    Ok(json!({ "x": q.x, "y": q.y, "text": format!("{}, {}", format_number(q.x, 3), format_number(q.y, 3)) }))
}

/// Where a point of a view (model mm) is on the sheet.
fn drw_to_sheet(s: &mut DrwSession, _k: Option<&mut dyn Kernel>, p: &Value) -> CmdResult {
    let vid = view_id(p, "view")?;
    let v = view(s.drawing(), vid)?;
    let g = s.current()?.view(vid).ok_or("the view is not computed")?;
    let q = to_sheet(v, g, vec2(field(p, "at")?, "at")?);
    Ok(json!({ "x": q.x, "y": q.y, "at": v2(q) }))
}

static COMMANDS: &[DrwCommand] = &[
    DrwCommand {
        id: "drw.info",
        label: "Drawing Info",
        help: "sheets, views (with their geometry counts) and annotations, dimension values and table rows as they are now",
        mutates: false,
        views: false,
        run: drw_info,
    },
    DrwCommand {
        id: "drw.pick",
        label: "Pick Geometry",
        help: "view; at: [x, y] (the edge drawn nearest that sheet point, as a click picks it) or view_at: [x, y] (the same, in the view, model mm), or edge: [face origin, face origin] or {\"body\", \"index\"} with component (assembly views); point: start | end | mid | center | whole (default). Returns a pick for drw.dimension, with its kind (point, line with its length in the view, circle with its diameter) and where it is on the sheet",
        mutates: false,
        views: true,
        run: drw_pick,
    },
    DrwCommand {
        id: "drw.view.projected",
        label: "Projected View",
        help: "parent (view); side: right | left | above | below | above_right | above_left | below_right | below_left (diagonals: isometric); at: [x, y] (sheet mm); scale",
        mutates: true,
        views: false,
        run: drw_projected,
    },
    DrwCommand {
        id: "drw.view.section",
        label: "Section View",
        help: "parent; a, b: the section line in the parent view (model mm, see drw.to_view); flip: look the other way; at; scale",
        mutates: true,
        views: false,
        run: drw_section,
    },
    DrwCommand {
        id: "drw.view.detail",
        label: "Detail View",
        help: "parent; center: [x, y] in the parent view (model mm); radius (model mm); scale (default twice the parent's); at",
        mutates: true,
        views: false,
        run: drw_detail,
    },
    DrwCommand {
        id: "drw.view.edit",
        label: "Edit View",
        help: "view; at: [x, y] or by: [dx, dy] (the views projected from it move along; a view projected beside, above or below its parent only slides in line with it); scale; hidden, tangent, centerlines, label: true or false",
        mutates: true,
        views: false,
        run: drw_view_edit,
    },
    DrwCommand {
        id: "drw.view.delete",
        label: "Delete View",
        help: "view (and the views made from it, and their annotations)",
        mutates: true,
        views: false,
        run: drw_view_delete,
    },
    DrwCommand {
        id: "drw.dimension",
        label: "Dimension",
        help: "view; type: horizontal | vertical | aligned | diameter | radius | angle; a, b: picks from drw.pick (a line alone for its length, a circle for its size); at: [x, y] where the dimension line or text goes (sheet mm), or by: [dx, dy] from the middle of what is measured; text (\"<>\" is the value); precision (decimals, default 2)",
        mutates: true,
        views: true,
        run: drw_dimension,
    },
    DrwCommand {
        id: "drw.center_mark",
        label: "Center Mark",
        help: "view; a: a pick of a circle or arc (drw.pick). A cross at its centre, kept on it as the model changes",
        mutates: true,
        views: true,
        run: drw_center_mark,
    },
    DrwCommand {
        id: "drw.centerline",
        label: "Centerline",
        help: "view; a, b: picks; the line through their points (a circle's centre, a line's middle, or the point picked), a little past both, kept on them as the model changes",
        mutates: true,
        views: true,
        run: drw_centerline,
    },
    DrwCommand {
        id: "drw.centerline.bisector",
        label: "Centerline Bisector",
        help: "view; a, b: picks of two straight edges; the centre line of the feature between them (parallel: midway, along both; meeting: the bisector of their angle), kept on them as the model changes",
        mutates: true,
        views: true,
        run: drw_centerline_bisector,
    },
    DrwCommand {
        id: "drw.dimension.suggest",
        label: "Suggest Dimensions",
        help: "view (base, projected or section): the dimensions a drafter would put on it first (overall width and height, each size of hole and round with its count), without what its dimensions already give; nothing is added",
        mutates: false,
        views: true,
        run: drw_dimension_suggest,
    },
    DrwCommand {
        id: "drw.dimension.auto",
        label: "Auto Dimension",
        help: "view; accept: [numbers from drw.dimension.suggest] (default all): adds them as dimensions, in one undo step",
        mutates: true,
        views: true,
        run: drw_dimension_auto,
    },
    DrwCommand {
        id: "drw.hole_table",
        label: "Hole Table",
        help: "view (a part view); origin: a pick for the datum (default the view's bottom-left); at: the table's top-left (default top right of the sheet)",
        mutates: true,
        views: true,
        run: drw_hole_table,
    },
    DrwCommand {
        id: "drw.parts_list",
        label: "Parts List",
        help: "view (an assembly view); at: the table's top-left (default above the title block)",
        mutates: true,
        views: true,
        run: drw_parts_list,
    },
    DrwCommand {
        id: "drw.balloon",
        label: "Balloon",
        help: "view; component, or attach_at: a sheet point on an edge of it (the balloon's leader ends there); at: where the balloon goes (default out from the view)",
        mutates: true,
        views: true,
        run: drw_balloon,
    },
    DrwCommand {
        id: "drw.balloon.auto",
        label: "Auto Balloon",
        help: "view: a balloon on one component of each part not ballooned yet",
        mutates: true,
        views: true,
        run: drw_balloon_auto,
    },
    DrwCommand {
        id: "drw.note",
        label: "Text",
        help: "text; at: [x, y] (sheet mm); sheet; height (mm, default 3.5)",
        mutates: true,
        views: false,
        run: drw_note,
    },
    DrwCommand {
        id: "drw.annotation.edit",
        label: "Edit Annotation",
        help: "annotation; by: [dx, dy] (move); text (dimension text, \"<>\" the value, null for the value alone; or a note's text); precision",
        mutates: true,
        views: false,
        run: drw_annotation_edit,
    },
    DrwCommand { id: "drw.delete", label: "Delete Annotation", help: "annotation", mutates: true, views: false, run: drw_delete },
    DrwCommand {
        id: "drw.props",
        label: "Drawing Properties",
        help: "title, number, revision, company, drawn_by, date (the title block's fields)",
        mutates: true,
        views: false,
        run: drw_props,
    },
    DrwCommand {
        id: "drw.sheet.add",
        label: "New Sheet",
        help: "size: A | B | C | D | A4 | A3 | A2 | A1 (default the standard's)",
        mutates: true,
        views: false,
        run: drw_sheet_add,
    },
    DrwCommand { id: "drw.sheet.size", label: "Sheet Size", help: "sheet, size", mutates: true, views: false, run: drw_sheet_size },
    DrwCommand {
        id: "drw.to_view",
        label: "Sheet to View",
        help: "view; at: [x, y] on the sheet. Returns where that is in the view (model mm)",
        mutates: false,
        views: true,
        run: drw_to_view,
    },
    DrwCommand {
        id: "drw.to_sheet",
        label: "View to Sheet",
        help: "view; at: [x, y] in the view (model mm). Returns where that is on the sheet",
        mutates: false,
        views: true,
        run: drw_to_sheet,
    },
    DrwCommand { id: "drw.undo", label: "Undo", help: "", mutates: false, views: false, run: drw_undo },
    DrwCommand { id: "drw.redo", label: "Redo", help: "", mutates: false, views: false, run: drw_redo },
];

/// The drawing commands.
pub fn commands() -> &'static [DrwCommand] {
    COMMANDS
}

pub fn find(id: &str) -> Option<&'static DrwCommand> {
    COMMANDS.iter().find(|c| c.id == id)
}

/// Runs a drawing command. With a kernel, the views are computed first when out of date.
pub fn run(s: &mut DrwSession, spec: &DrwCommand, params: &Value, kernel: Option<&mut dyn Kernel>) -> CmdResult {
    let params = if params.is_null() { &Value::Object(Default::default()) } else { params };
    if !params.is_object() {
        return Err("parameters must be a JSON object".into());
    }
    match kernel {
        Some(k) => {
            s.refresh(k);
            let r = (spec.run)(s, Some(&mut *k), params);
            // Answers about the drawing reflect the change at once.
            s.refresh(k);
            r
        }
        None if spec.views && s.evaluation().is_none() => Err(CmdError(format!("{} needs the views computed", spec.id))),
        None => (spec.run)(s, None, params),
    }
}
